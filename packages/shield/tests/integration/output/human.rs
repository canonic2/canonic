//! Human output of every public command, through a real controlling terminal
//! with shared TUI prompts: settled reviews, receipts, empty states and errors.
use assert_fs::TempDir;
use serde_json::{Value, json};
use std::{fmt::Write, path::Path};

/// Runs one command in a PTY with input echo off and returns what a person
/// sees: the terminal transcript (prompts and reviews), stdout, stderr and the
/// exit status.
fn screen(root: &Path, args: &[&str], answers: &str) -> String {
    let binary = assert_cmd::cargo::cargo_bin!("shield");
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/operator_prompts.py"
    );
    let mut full: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    full.extend(["--no-debug-log".into(), "--project".into()]);
    full.push(root.to_string_lossy().into_owned());
    if args.first() == Some(&"init") {
        full.push("--cache-dir".into());
        full.push(root.join(".shield/cache").to_string_lossy().into_owned());
    }
    let output = std::process::Command::new("python3")
        .arg(script)
        .arg(binary)
        .arg(json!(full).to_string())
        .arg(answers)
        .env("HOME", root)
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .output()
        .expect("PTY harness runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("PTY harness receipt");
    let mut text = format!("$ shield {}\n", args.join(" "));
    for stream in ["terminal", "stdout", "stderr"] {
        let content = result[stream]
            .as_str()
            .unwrap_or_default()
            .replace('\r', "");
        if !content.trim().is_empty() {
            let _ = writeln!(text, "--- {stream}\n{}", content.trim_end());
        }
    }
    let _ = writeln!(text, "--- exit {}", result["code"]);
    text
}

/// Snapshot filters for temporary paths, digests, selectors and times.
fn filters(paths: &[(&Path, &str)]) -> Vec<(String, String)> {
    let mut filters: Vec<(String, String)> = paths
        .iter()
        .flat_map(|(path, name)| {
            let canonical = path.canonicalize().expect("fixture path");
            [
                canonical.to_string_lossy().into_owned(),
                path.to_string_lossy().into_owned(),
            ]
            .map(|p| (regex::escape(&p), (*name).to_owned()))
        })
        .collect();
    filters.push((
        format!(
            "requires Shield {}",
            regex::escape(env!("CARGO_PKG_VERSION"))
        ),
        "requires Shield [SHIELD-VERSION]".into(),
    ));
    let binary = assert_cmd::cargo::cargo_bin!("shield")
        .canonicalize()
        .expect("built Shield executable");
    filters.push((
        format!("'?{}'?", regex::escape(&binary.to_string_lossy())),
        "[SHIELD]".to_owned(),
    ));
    filters.extend(
        [
            (r"pack:[0-9a-f-]{36}", "pack:[SELECTOR]"),
            (r"\b[0-9a-f]{64}\b", "[DIGEST]"),
            (r"\b[0-9a-f]{12}\b", "[SHORT-DIGEST]"),
            (r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z", "[TIME]"),
            (r"(?m)[ \t]+$", ""),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned())),
    );
    filters
}

#[test]
fn every_public_command_has_concise_human_output() {
    let root = TempDir::new().expect("project");
    let registry = super::super::operator::local_dependency_source();
    let source = registry.path().to_str().expect("registry path");
    let mut steps = public_command_steps(source);
    if cfg!(not(target_os = "macos")) {
        steps.retain(|(name, _, _)| *name != "cache-prepare");
    }
    let filters = filters(&[(root.path(), "[PROJECT]"), (registry.path(), "[REGISTRY]")]);
    for (name, args, answers) in steps {
        let text = screen(root.path(), &args, answers);
        insta::with_settings!({filters => filters.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(format!("human-{name}"), text);
        });
    }
}

/// Each public command in a project's life, with its snapshot name and answers.
#[expect(
    clippy::too_many_lines,
    reason = "one row per public command keeps the project's sequence in one place"
)]
fn public_command_steps(source: &str) -> Vec<(&'static str, Vec<&str>, &'static str)> {
    vec![
        ("uninitialized-status", vec!["status"], ""),
        (
            "init-cancel",
            vec!["init", "--no-setup", "--no-baseline"],
            "no\n",
        ),
        ("init", vec!["init", "--no-setup", "--no-baseline"], "yes\n"),
        ("status", vec!["status"], ""),
        ("doctor", vec!["doctor"], ""),
        ("policy-review-empty", vec!["policy", "review"], "\n"),
        ("policy-show-empty", vec!["policy", "show"], ""),
        ("rule-list-empty", vec!["rule", "list"], ""),
        ("pack-list-empty", vec!["pack", "list"], ""),
        ("request-list-empty", vec!["request", "list"], ""),
        ("history-list-empty", vec!["history", "list"], ""),
        ("registry-list-builtin", vec!["registry", "list"], ""),
        (
            "registry-add",
            vec!["registry", "add", "acme", source],
            "yes\n",
        ),
        (
            "registry-add-duplicate",
            vec!["registry", "add", "acme", source],
            "",
        ),
        ("registry-list", vec!["registry", "list"], ""),
        (
            "registry-default",
            vec!["registry", "default", "acme"],
            "yes\n",
        ),
        ("pack-search", vec!["pack", "search", "acme/"], ""),
        ("pack-search-none", vec!["pack", "search", "missing"], ""),
        ("pack-info", vec!["pack", "info", "acme/parent"], ""),
        (
            "pack-info-missing",
            vec!["pack", "info", "acme/missing"],
            "",
        ),
        ("pack-info-rules", vec!["pack", "info", "acme/test"], ""),
        ("pack-add", vec!["pack", "add", "acme/parent"], "\nyes\n"),
        ("pack-add-missing", vec!["pack", "add", "acme/missing"], ""),
        ("pack-list", vec!["pack", "list"], ""),
        ("pack-show", vec!["pack", "show", "acme/parent"], ""),
        (
            "pack-show-missing",
            vec!["pack", "show", "acme/missing"],
            "",
        ),
        ("pack-update-unchanged", vec!["pack", "update"], ""),
        (
            "pack-add-profile",
            vec!["pack", "add", "acme/parent", "--profile", "strict"],
            "yes\n",
        ),
        (
            "pack-remove-required",
            vec!["pack", "remove", "acme/test"],
            "",
        ),
        ("policy-review", vec!["policy", "review"], "\n"),
        ("policy-show", vec!["policy", "show"], ""),
        ("policy-repair", vec!["policy", "review"], "repair\nyes\n"),
        (
            "policy-history",
            vec!["policy", "review"],
            "history\n\n1000\n\nyes\n",
        ),
        (
            "rule-add",
            vec!["rule", "add"],
            "status\nargv\ngit\n[\"status\"]\nallow\nRead repository state\n\nyes\n",
        ),
        ("rule-list", vec!["rule", "list"], ""),
        ("rule-remove", vec!["rule", "remove", "status"], "yes\n"),
        (
            "pack-remove",
            vec!["pack", "remove", "acme/parent"],
            "yes\n",
        ),
        (
            "registry-remove",
            vec!["registry", "remove", "acme"],
            "yes\n",
        ),
        (
            "request-approve-missing",
            vec!["request", "approve", "missing"],
            "",
        ),
        (
            "history-show-missing",
            vec!["history", "show", "missing"],
            "",
        ),
        ("cache-prepare", vec!["cache", "prepare"], ""),
        (
            "mcp-setup-claude",
            vec!["mcp", "setup", "--agent", "claude"],
            "\nyes\n",
        ),
        (
            "mcp-setup-dry-run",
            vec!["mcp", "setup", "--agent", "codex", "--dry-run"],
            "",
        ),
        (
            "mcp-setup-unchanged",
            vec!["mcp", "setup", "--agent", "claude"],
            "\nyes\n",
        ),
        ("status-integrated", vec!["status"], ""),
        (
            "mcp-setup-remove",
            vec!["mcp", "setup", "--remove", "--agent", "claude"],
            "yes\n",
        ),
    ]
}

/// Approval requests and request history need a real Ask decision for a
/// native launch, which only macOS supports.
#[test]
#[cfg(target_os = "macos")]
fn approvals_and_history_have_concise_human_output() {
    use assert_fs::prelude::*;
    let root = TempDir::new().expect("project");
    let rule = json!({"id":"echo","decision":"ask","reason":"Acme echo needs approval","conditions":{"domain":"argv","executable":"/bin/echo","arguments":{"kind":"any"}}});
    root.child(".shield/config.yml")
        .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#)
        .expect("config");
    root.child(".shield/rules.yml")
        .write_str(&json!({"schema_version":1,"rules":[rule],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string())
        .expect("rules");
    let init = screen(
        root.path(),
        &["init", "--no-setup", "--no-baseline"],
        "yes\n",
    );
    assert!(init.contains("--- exit 0"), "{init}");
    let run = |args: &[&str]| {
        assert_cmd::cargo::cargo_bin_cmd!("shield")
            .args(["__internal", "run", "--project"])
            .arg(root.path())
            .arg("--")
            .args(args)
            .env("HOME", root.path())
            .output()
            .expect("native run")
    };
    assert_eq!(run(&["/bin/echo", "Acme"]).status.code(), Some(3));
    let structured = |args: &[&str]| -> Value {
        let output = assert_cmd::cargo::cargo_bin_cmd!("shield")
            .args(args)
            .args(["--json", "--project"])
            .arg(root.path())
            .env("HOME", root.path())
            .output()
            .expect("inspection");
        serde_json::from_slice(&output.stdout).expect("JSON")
    };
    let request = structured(&["request", "list"])["requests"][0]["request_id"]
        .as_str()
        .expect("request")
        .to_owned();
    assert_eq!(structured(&["status"])["pending_approvals"], 1);
    let status = screen(root.path(), &["status"], "");
    assert!(
        status.contains("1 action request is waiting: review with shield request list."),
        "{status}"
    );
    let filters = filters(&[(root.path(), "[PROJECT]")]);
    let mut steps = vec![
        ("request-list", vec!["request", "list"], ""),
        (
            "request-show",
            vec!["request", "show", request.as_str()],
            "",
        ),
        (
            "request-approve",
            vec!["request", "approve", request.as_str()],
            "yes\n",
        ),
    ];
    let mut texts = Vec::new();
    for (name, args, answers) in steps.drain(..) {
        texts.push((name, screen(root.path(), &args, answers)));
    }
    let retried = assert_cmd::cargo::cargo_bin_cmd!("shield")
        .args(["__internal", "run", "--project"])
        .arg(root.path())
        .args([
            "--approval-request",
            request.as_str(),
            "--",
            "/bin/echo",
            "Acme",
        ])
        .env("HOME", root.path())
        .output()
        .expect("approved run");
    assert!(retried.status.success());
    let entry = structured(&["history", "list"])["entries"][0]["entry_id"]
        .as_str()
        .expect("history entry")
        .to_owned();
    for (name, args) in [
        ("history-list", vec!["history", "list"]),
        ("history-show", vec!["history", "show", entry.as_str()]),
        (
            "request-deny-consumed",
            vec!["request", "deny", request.as_str()],
        ),
    ] {
        texts.push((name, screen(root.path(), &args, "")));
    }
    let mut filters = filters;
    filters.insert(0, (regex::escape(&request), "[REQUEST]".into()));
    filters.insert(0, (regex::escape(&entry), "[ENTRY]".into()));
    for (name, text) in texts {
        insta::with_settings!({filters => filters.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(format!("human-{name}"), text);
        });
    }
}

/// Submits one proposal through a real MCP session, as an agent would. Input
/// stays open until the reply arrives: closing it refuses queued changes.
fn propose(root: &Path, change: &Value) {
    use std::io::{BufRead, Write as _};
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"policy_propose","arguments":{"change":change,"agent_reason":"Read the Acme repository state."}}}),
    ];
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("shield"))
        .args(["mcp", "serve", "--project"])
        .arg(root)
        .env("HOME", root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("MCP session runs");
    let mut stdin = child.stdin.take().expect("piped MCP input");
    for message in messages {
        writeln!(stdin, "{message}").expect("write MCP message");
    }
    stdin.flush().expect("flush MCP input");
    let replies = std::io::BufReader::new(child.stdout.take().expect("piped MCP output"))
        .lines()
        .map(|line| serde_json::from_str::<Value>(&line.expect("MCP output line")).expect("JSON"))
        .find(|reply| reply["id"] == 2)
        .expect("proposal reply");
    drop(stdin);
    assert!(child.wait().expect("MCP session ends").success());
    assert!(replies.to_string().contains("proposal_id"), "{replies}");
}

#[test]
fn pending_proposals_are_reviewed_from_shield_policy_review() {
    let root = TempDir::new().expect("project");
    screen(
        root.path(),
        &["init", "--no-setup", "--no-baseline"],
        "yes\n",
    );
    let rule = |id: &str, arguments: &[&str]| {
        json!({"kind":"rule_add","rule":{"id":id,"decision":"allow","reason":"Acme repository state",
            "conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":arguments}}}})
    };
    propose(root.path(), &rule("status", &["status"]));
    propose(root.path(), &rule("log", &["log"]));
    let filters = filters(&[(root.path(), "[PROJECT]")]);
    // Enter opens the waiting proposals; each is answered by name, no IDs typed.
    for (name, answers) in [
        ("policy-proposals", "\nreject\naccept\n"),
        ("policy-proposals-done", "\n"),
    ] {
        let text = screen(root.path(), &["policy", "review"], answers);
        insta::with_settings!({filters => filters.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(format!("human-{name}"), text);
        });
    }
}
