//! Public workflows use a real controlling terminal, without a confirmation bypass.
use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::{TempDir, prelude::*};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[test]
fn human_errors_are_actionable_plain_text_and_json_errors_keep_their_contract() {
    let root = TempDir::new().expect("unenrolled project");
    let human = cargo_bin_cmd!("shield")
        .args(["status", "--no-debug-log", "--project"])
        .arg(root.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("human error");
    assert_eq!(human.status.code(), Some(1));
    assert_eq!(human.stdout.as_slice(), b"");
    let stderr = String::from_utf8(human.stderr).expect("human UTF8");
    assert!(stderr.contains("Error: Shield is not initialized here"));
    assert!(stderr.contains("shield init"));
    assert!(!stderr.contains("invalid review input or state"));
    assert!(!stderr.contains('\u{1b}'));
    let structured = cargo_bin_cmd!("shield")
        .args(["status", "--json", "--no-debug-log", "--project"])
        .arg(root.path())
        .output()
        .expect("JSON error");
    assert_eq!(structured.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&structured.stdout).expect("error envelope");
    assert_eq!(result["error"]["code"], "invalid-review");
    assert_eq!(
        result["error"]["message"],
        "invalid review input or state: no enrolled project; run shield init"
    );
    assert_eq!(
        result["error"]["details"],
        "No enrolled project was found for the selected directory."
    );
    assert_eq!(
        result["error"]["recovery"][0],
        "Run shield init in the intended project."
    );
    assert_eq!(
        result["error"]["recovery"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(structured.stderr.as_slice(), b"");
    assert!(stderr.lines().count() <= 4, "{stderr}");
}

#[test]
#[cfg(unix)]
fn onboarding_selects_both_agents_together_and_skip_leaves_integrations_absent() {
    let root = TempDir::new().expect("project");
    assert_eq!(
        guided(&root, &["init"], "claude,codex\n\nno\n")["code"],
        130
    );
    assert!(!root.path().join(".shield/review/state.json").exists());
    assert!(!root.path().join(".claude").exists());
    assert!(!root.path().join(".codex").exists());
    let result = guided(&root, &["init"], "claude,codex\n\nyes\n");
    assert_eq!(result["code"], 0, "{result}");
    let saved: Value = serde_json::from_slice(
        &std::fs::read(root.path().join(".shield/review/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert_eq!(
        saved["operator"]["integration"]["agents"]
            .as_object()
            .expect("agents")
            .len(),
        2
    );
    assert_eq!(
        saved["operator"]["integration"]["ask_fallback"]["codex"],
        "allow"
    );
    // Reproduce a removed enrollment with surviving generated host files.
    let paths = [
        ".mcp.json",
        ".claude/settings.local.json",
        ".codex/hooks.json",
        ".codex/config.toml",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| std::fs::read(root.path().join(p)).expect("host file"))
        .collect();
    std::fs::remove_dir_all(root.path().join(".shield/review")).expect("remove fixture enrollment");
    assert_eq!(
        guided(&root, &["init"], "claude,codex\n\nno\n")["code"],
        130
    );
    assert!(!root.path().join(".shield/review/state.json").exists());
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(
            std::fs::read(root.path().join(path)).expect("preserved host file"),
            before[index]
        );
    }
    let reclaimed = guided(&root, &["init"], "claude,codex\n\nyes\n");
    assert_eq!(reclaimed["code"], 0, "{reclaimed}");
    assert_eq!(query(&root, &["doctor"])["repair_needed"], false);
    let skipped = TempDir::new().expect("skipped project");
    assert_eq!(guided(&skipped, &["init"], "\nyes\n")["code"], 0);
    assert!(!skipped.path().join(".claude").exists());
    assert!(!skipped.path().join(".codex").exists());
    assert!(!skipped.path().join(".mcp.json").exists());
}

#[cfg(unix)]
pub(super) fn interactive(root: &TempDir, args: &[&str], answers: &str) -> Value {
    let mut selected = args.to_vec();
    if selected.first().is_some_and(|a| *a == "init") && !selected.contains(&"--agent") {
        selected.push("--no-setup");
    }
    guided(root, &selected, answers)
}

#[cfg(unix)]
fn guided(root: &TempDir, args: &[&str], answers: &str) -> Value {
    let binary = assert_cmd::cargo::cargo_bin!("shield");
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/operator_prompts.py"
    );
    let mut args: Vec<_> = args.iter().map(|s| (*s).to_owned()).collect();
    // These fixtures supply their own policy and must not depend on hosted discovery.
    if args.first().is_some_and(|a| a == "init") && !args.iter().any(|a| a == "--no-baseline") {
        args.push("--no-baseline".into());
    }
    args.extend([
        "--project".into(),
        root.path().to_string_lossy().into_owned(),
    ]);
    if args.first().is_some_and(|a| a == "init") {
        args.extend([
            "--cache-dir".into(),
            root.path()
                .join(".shield/cache")
                .to_string_lossy()
                .into_owned(),
        ]);
    }
    let output = std::process::Command::new("python3")
        .arg(script)
        .arg(binary)
        .arg(json!(args).to_string())
        .arg(answers)
        .arg("operator")
        .env("HOME", root.path())
        .env("TERM", "xterm-256color")
        .env("NO_COLOR", "1")
        .output()
        .expect("PTY harness runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("PTY harness receipt")
}
pub(super) fn query(root: &TempDir, args: &[&str]) -> Value {
    let output = cargo_bin_cmd!("shield")
        .args(args)
        .args(["--json", "--project"])
        .arg(root.path())
        .env("HOME", root.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("structured public inspection")
}

#[test]
#[cfg(unix)]
fn guided_mcp_setup_cancellation_repair_and_removal_preserve_project_policy() {
    let root = TempDir::new().expect("project");
    root.child(".mcp.json")
        .write_str(r#"{"mcpServers":{"acme":{"command":"acme"}}}"#)
        .expect("other server");
    assert_eq!(
        interactive(&root, &["init", "--agent", "claude"], "\nno\n")["code"],
        130
    );
    assert!(!root.path().join(".shield/review/state.json").exists());
    // Enter on the permissions page keeps the default approvals.
    assert_eq!(guided(&root, &["init"], "claude\n\nyes\n")["code"], 0);
    let before = query(&root, &["status"]);
    assert_eq!(
        before["coverage"]["registered_integrations"][0]["status"],
        "registered"
    );
    assert_eq!(
        before["coverage"]["installed_integrations"][0]["surface"],
        "native-hooks"
    );
    let original = std::fs::read(root.path().join(".mcp.json")).expect("MCP bytes");
    assert_eq!(
        interactive(&root, &["mcp", "setup", "--agent", "claude"], "\nyes\n")["code"],
        0
    );
    assert_eq!(
        std::fs::read(root.path().join(".mcp.json")).expect("MCP bytes"),
        original
    );
    let mut settings: Value = serde_json::from_slice(&original).expect("MCP config");
    settings["mcpServers"]
        .as_object_mut()
        .expect("servers")
        .remove("shield");
    std::fs::write(root.path().join(".mcp.json"), settings.to_string()).expect("manual deletion");
    let persisted = std::fs::read(root.path().join(".shield/review/state.json")).expect("state");
    assert_eq!(query(&root, &["doctor"])["repair_needed"], true);
    assert_eq!(
        persisted,
        std::fs::read(root.path().join(".shield/review/state.json")).expect("state")
    );
    assert_eq!(
        interactive(&root, &["mcp", "setup", "--agent", "claude"], "\nyes\n")["code"],
        0
    );
    assert_eq!(query(&root, &["doctor"])["repair_needed"], false);
    let remove = ["mcp", "setup", "--remove", "--agent", "claude"];
    assert_eq!(interactive(&root, &remove, "no\n")["code"], 130);
    assert_eq!(
        query(&root, &["status"])["coverage"]["registered_integrations"]
            .as_array()
            .expect("registrations")
            .len(),
        1
    );
    assert_eq!(interactive(&root, &remove, "yes\n")["code"], 0);
    let after = query(&root, &["status"]);
    assert_eq!(after["active"], before["active"]);
    assert_eq!(after["coverage"]["registered_integrations"], json!([]));
    let remaining: Value =
        serde_json::from_slice(&std::fs::read(root.path().join(".mcp.json")).expect("MCP config"))
            .expect("MCP JSON");
    assert_eq!(remaining["mcpServers"], json!({"acme":{"command":"acme"}}));
}

#[test]
#[cfg(unix)]
fn invalid_mcp_setup_and_json_requests_never_enroll_or_modify_a_project() {
    let root = TempDir::new().expect("project");
    root.child(".mcp.json")
        .write_str("broken")
        .expect("malformed settings");
    assert_eq!(
        interactive(&root, &["init", "--agent", "claude"], "yes\n")["code"],
        1
    );
    assert!(!root.path().join(".shield/review/state.json").exists());
    cargo_bin_cmd!("shield")
        .args(["init", "--agent", "claude", "--json", "--project"])
        .arg(root.path())
        .assert()
        .failure();
    assert_eq!(
        std::fs::read_to_string(root.path().join(".mcp.json")).expect("preserved file"),
        "broken"
    );
}

#[test]
#[cfg(unix)]
fn setup_defaults_to_allow_and_preserves_saved_fallback_without_reinitializing_policy() {
    let root = TempDir::new().expect("project");
    cargo_bin_cmd!("shield")
        .args(["mcp", "setup", "--agent", "codex", "--project"])
        .arg(root.path())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("shield init"));
    assert!(!root.path().join(".shield/review/state.json").exists());
    assert_eq!(guided(&root, &["init"], "codex\n\nyes\n")["code"], 0);
    let before = query(&root, &["status"]);
    assert_eq!(
        before["coverage"]["installed_integrations"][0]["ask_fallback"],
        "allow"
    );
    let state_path = root.path().join(".shield/review/state.json");
    let bytes = std::fs::read(&state_path).unwrap();
    let codex = ["mcp", "setup", "--agent", "codex"];
    assert_eq!(interactive(&root, &codex, "\nno\n")["code"], 130);
    assert_eq!(std::fs::read(&state_path).unwrap(), bytes);
    let cancelled_cache = root.path().join("cancelled-cache");
    assert_eq!(
        interactive(
            &root,
            &[
                "mcp",
                "setup",
                "--agent",
                "codex",
                "--cache-dir",
                cancelled_cache.to_str().unwrap()
            ],
            "\nno\n"
        )["code"],
        130
    );
    assert_eq!(std::fs::read(&state_path).unwrap(), bytes);
    assert!(!cancelled_cache.exists());
    assert_eq!(interactive(&root, &codex, "\nyes\n")["code"], 0);
    assert_eq!(
        query(&root, &["status"])["coverage"]["installed_integrations"][0]["ask_fallback"],
        "allow"
    );
    assert_eq!(
        interactive(&root, &["mcp", "setup", "--agent", "claude"], "\nyes\n")["code"],
        0
    );
    let after_add = query(&root, &["status"]);
    assert_eq!(after_add["active"], before["active"]);
    assert_eq!(interactive(&root, &["init"], "")["code"], 0);
    assert_eq!(
        interactive(&root, &["init", "--agent", "codex"], "")["code"],
        1
    );
    cargo_bin_cmd!("shield")
        .args(["mcp", "setup", "--agent", "codex", "--json", "--project"])
        .arg(root.path())
        .assert()
        .code(1);
    // Represent a project whose operator previously confirmed Block. A later
    // setup must refresh the integration without expanding that authorization.
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    saved["operator"]["integration"]["ask_fallback"]["codex"] = json!("block");
    std::fs::write(&state_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(interactive(&root, &codex, "\nyes\n")["code"], 0);
    let after = query(&root, &["status"]);
    assert_eq!(after["active"], before["active"]);
    assert_eq!(
        after["coverage"]["installed_integrations"][1]["ask_fallback"],
        "block"
    );
    assert_eq!(
        after["coverage"]["installed_integrations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
#[cfg(unix)]
fn global_sources_work_before_enrollment_and_project_defaults_take_precedence() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    source
        .child("catalog.json")
        .write_str(r#"{"schema_version":1,"packs":[]}"#)
        .expect("empty catalog");
    let path = source.path().to_str().expect("source path");
    let added = interactive(
        &root,
        &["registry", "add", "acme", path, "--scope", "user"],
        "yes\n",
    );
    assert_eq!(added["code"], 0, "{added}");
    assert!(!root.path().join(".shield/review").exists());
    assert_eq!(
        interactive(
            &root,
            &["registry", "default", "acme", "--scope", "user"],
            "yes\n"
        )["code"],
        0
    );
    let user = ["registry", "list", "--scope", "user"];
    assert_eq!(query(&root, &user)["default"], "acme");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    assert_eq!(query(&root, &["registry", "list"])["default"], "acme");
    assert_eq!(
        interactive(&root, &["registry", "default", "main"], "yes\n")["code"],
        0
    );
    assert_eq!(query(&root, &["registry", "list"])["default"], "main");
    assert_eq!(query(&root, &user)["default"], "acme");
}

#[test]
#[cfg(unix)]
fn policy_repair_recreates_missing_files_without_using_init_or_changing_active_policy() {
    let root = TempDir::new().expect("project");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let before = query(&root, &["status"]);
    let config = root.path().join(".shield/config.yml");
    let original = std::fs::read(&config).unwrap();
    std::fs::remove_file(&config).unwrap();
    assert_eq!(query(&root, &["status"])["repair_needed"], true);
    let review = ["policy", "review"];
    assert_eq!(interactive(&root, &review, "repair\nno\n")["code"], 130);
    assert!(!config.exists());
    assert_eq!(interactive(&root, &review, "repair\nyes\n")["code"], 0);
    assert_eq!(std::fs::read(config).unwrap(), original);
    assert_eq!(query(&root, &["status"])["active"], before["active"]);
}

#[test]
#[cfg(unix)]
fn guided_add_asks_setup_questions_in_declaration_order_and_reasks_refused_values() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    // Raw text keeps `profile` before `endpoint`; by name, endpoint would come first.
    let pack = r#"{"schema_version":1,"kind":"pack","id":"acme/cloud","version":"0.1.0","description":"Acme cloud","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],
        "variables":{
          "profile":{"type":"string","description":"Read-only profile","max_length":20,"setup":{"question":"Restrict the profile?","unset":"Any profile","value":"Only this profile","placeholder":"acme-read-only"}},
          "endpoint":{"type":"string","description":"Endpoint","setup":{"unset":"Any endpoint","ask_when":[{"var":"profile","set":true}]}}},
        "rules":[{"id":"cloud","decision":"allow","conditions":{"domain":"argv","executable":"acme-cloud","arguments":{"kind":"sequence","prefix":[{"var":"profile"}],"rest":{"kind":"any"}}}}]}"#;
    source.child("pack.json").write_str(pack).expect("artifact");
    let digest = Sha256::digest(pack.as_bytes())
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/cloud","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let source_path = source.path().to_str().expect("fixture path");
    let registered = interactive(&root, &["registry", "add", "acme", source_path], "yes\n");
    assert_eq!(registered["code"], 0, "{registered}");
    // Select the typed row, observe validation refusal, then replace its retained
    // text in the same form. Endpoint: unset. Each later item awaits a new prompt.
    // Then keep the reviewed answers, save as the pack suggests and confirm.
    let answers = format!(
        "@{}",
        json!([
            {"keys":"2acme-read-only-profile-too-long\r"},
            {"wait":"variable text is empty or too long", "keys":"\r\u{7}acme-read-only\r"},
            {"keys":"1"}, {"keys":"\r"}, {"keys":"\r"}, {"keys":"2\r"}
        ])
    );
    let added = interactive(
        &root,
        &[
            "pack",
            "add",
            "acme/cloud@0.1.0",
            "--profile",
            "recommended",
        ],
        &answers,
    );
    assert_eq!(added["code"], 0, "{added}");
    let config = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let config: Value = serde_saphyr::from_str(&config).expect("config YAML");
    let selection = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|p| p["id"] == "acme/cloud")
        .expect("selection");
    assert_eq!(selection["variables"], json!({"profile": "acme-read-only"}));
    // Keep both answers, use them, save as suggested, then mark the rule. Escape
    // in its step returns to the list with the mark kept, so Enter there opens
    // the rule again, where Off is preselected.
    let answers = format!(
        "@{}",
        json!([
            {"keys":"\r"}, {"keys":"\r"}, {"keys":"\r"}, {"keys":"\r"},
            {"keys":" \r"}, {"keys":"\u{1b}"}, {"keys":"\r"}, {"keys":"\r"}, "yes"
        ])
    );
    let configured = interactive(&root, &["pack", "configure", "acme/cloud"], &answers);
    assert_eq!(configured["code"], 0, "{configured}");
    let config = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let config: Value = serde_saphyr::from_str(&config).expect("config YAML");
    let selection = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|p| p["id"] == "acme/cloud")
        .expect("selection");
    assert_eq!(selection["variables"], json!({"profile": "acme-read-only"}));
    assert_eq!(selection["overrides"], json!({"cloud": "off"}));
    // Back on the first question cancels and changes nothing.
    let before = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let cancelled = interactive(&root, &["pack", "configure", "acme/cloud"], "back\n");
    assert_eq!(cancelled["code"], 130, "{cancelled}");
    assert_eq!(
        std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config"),
        before
    );
    // Keep the profile, go back from the endpoint to unset it (the endpoint
    // question then no longer applies), go back from the rule list to the
    // profile, type another one, answer the endpoint, use and save the answers,
    // then confirm the still-marked rule and keep its override.
    let answers = format!(
        "@{}",
        json!([
            {"keys":"\r"}, {"keys":"\u{1b}"}, {"keys":"1"},
            {"keys":"\u{1b}"}, {"keys":"2acme-other\r"}, {"keys":"1"},
            {"keys":"\r"}, {"keys":"\r"}, {"keys":"\r"}, {"keys":"\r"}, {"keys":"2\r"}
        ])
    );
    let revised = interactive(&root, &["pack", "configure", "acme/cloud"], &answers);
    assert_eq!(revised["code"], 0, "{revised}");
    let config = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let config: Value = serde_saphyr::from_str(&config).expect("config YAML");
    let selection = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|p| p["id"] == "acme/cloud")
        .expect("selection");
    assert_eq!(selection["variables"], json!({"profile": "acme-other"}));
    assert_eq!(selection["overrides"], json!({"cloud": "off"}));
}

#[test]
#[cfg(unix)]
fn guided_rule_overrides_mark_a_filtered_rule_with_space() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    // Ten rules give the list a filter row.
    let rules: Vec<Value> = (0..10)
        .map(|n| json!({"id": format!("tool-{n}"), "decision": "allow", "conditions": {"domain": "argv", "executable": format!("acme-x{n}"), "arguments": {"kind": "any"}}}))
        .collect();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"0.1.0","description":"Acme tools","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":rules}).to_string();
    source
        .child("pack.json")
        .write_str(&pack)
        .expect("artifact");
    let digest = Sha256::digest(pack.as_bytes())
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let source_path = source.path().to_str().expect("fixture path");
    let registered = interactive(&root, &["registry", "add", "acme", source_path], "yes\n");
    assert_eq!(registered["code"], 0, "{registered}");
    let added = interactive(
        &root,
        &[
            "pack",
            "add",
            "acme/tools@0.1.0",
            "--profile",
            "recommended",
        ],
        "yes\n",
    );
    assert_eq!(added["code"], 0, "{added}");
    // Typing narrows the list to one rule; Space marks it with the filter
    // visible and Enter confirms the marked set, then Off is chosen for it.
    let answers = format!(
        "@{}",
        json!([
            {"keys":"tool-7 \r"}, {"keys":"\r"}, "yes"
        ])
    );
    let configured = interactive(&root, &["pack", "configure", "acme/tools"], &answers);
    assert_eq!(configured["code"], 0, "{configured}");
    let config = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let config: Value = serde_saphyr::from_str(&config).expect("config YAML");
    let selection = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|p| p["id"] == "acme/tools")
        .expect("selection");
    assert_eq!(selection["overrides"], json!({"tool-7": "off"}));
}

#[test]
#[cfg(unix)]
fn going_back_through_rule_overrides_keeps_an_existing_override() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    let rules: Vec<Value> = (0..2)
        .map(|n| json!({"id": format!("tool-{n}"), "decision": "allow", "conditions": {"domain": "argv", "executable": format!("acme-x{n}"), "arguments": {"kind": "any"}}}))
        .collect();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"0.1.0","description":"Acme tools","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":rules}).to_string();
    source
        .child("pack.json")
        .write_str(&pack)
        .expect("artifact");
    let digest = Sha256::digest(pack.as_bytes())
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let source_path = source.path().to_str().expect("fixture path");
    let registered = interactive(&root, &["registry", "add", "acme", source_path], "yes\n");
    assert_eq!(registered["code"], 0, "{registered}");
    let added = interactive(
        &root,
        &[
            "pack",
            "add",
            "acme/tools@0.1.0",
            "--profile",
            "recommended",
        ],
        "yes\n",
    );
    assert_eq!(added["code"], 0, "{added}");
    let denied = interactive(
        &root,
        &[
            "pack",
            "configure",
            "acme/tools",
            "--override",
            "tool-0=deny",
        ],
        "yes\n",
    );
    assert_eq!(denied["code"], 0, "{denied}");
    // Unmark tool-0, mark tool-1 and continue; go back from tool-1's step;
    // mark tool-0 again. Its step must still start on its existing Deny, so
    // Enter keeps it, and tool-1 takes the default Off.
    let answers = format!(
        "@{}",
        json!([
            {"keys":"\x1b[H \x1b[B \r"}, "back",
            {"keys":"\x1b[H \r"}, {"keys":"\r"}, {"keys":"\r"}, "yes"
        ])
    );
    let configured = interactive(&root, &["pack", "configure", "acme/tools"], &answers);
    assert_eq!(configured["code"], 0, "{configured}");
    let config = std::fs::read_to_string(root.path().join(".shield/config.yml")).expect("config");
    let config: Value = serde_saphyr::from_str(&config).expect("config YAML");
    let selection = config["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|p| p["id"] == "acme/tools")
        .expect("selection");
    assert_eq!(
        selection["overrides"],
        json!({"tool-0": "deny", "tool-1": "off"})
    );
}

#[test]
#[cfg(unix)]
fn local_registration_install_profile_change_and_offline_removal_are_atomic() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    let path = super::policy::candidate(&source);
    let bytes = std::fs::read_to_string(path.path()).expect("artifact");
    let digest = Sha256::digest(bytes.as_bytes())
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let source_path = source.path().to_str().expect("fixture path");
    let registered = interactive(&root, &["registry", "add", "acme", source_path], "yes\n");
    assert_eq!(registered["code"], 0, "{registered}");
    let installed = interactive(&root, &["pack", "add", "acme/test@0.1.0"], "loose\nyes\n");
    assert_eq!(installed["code"], 0, "{installed}");
    let packs = query(&root, &["pack", "list"]);
    assert_eq!(packs["packs"][0]["pinned"], true);
    assert_eq!(packs["packs"][0]["profile"], "loose");
    let selector = packs["packs"][0]["selector"]
        .as_str()
        .expect("stable selection");
    assert_eq!(
        interactive(&root, &["registry", "remove", "acme"], "yes\n")["code"],
        0
    );
    let changed = interactive(
        &root,
        &["pack", "add", selector, "--profile", "strict"],
        "yes\n",
    );
    assert_eq!(changed["code"], 0, "{changed}");
    assert_eq!(query(&root, &["pack", "list"])["packs"][0]["pinned"], true);
    let before = query(&root, &["status"])["active"].clone();
    assert_eq!(
        interactive(&root, &["pack", "remove", selector], "no\n")["code"],
        130
    );
    assert_eq!(query(&root, &["status"])["active"], before);
    assert_eq!(
        interactive(&root, &["pack", "remove", selector], "yes\n")["code"],
        0
    );
    assert_eq!(
        query(&root, &["pack", "list"])["packs"]
            .as_array()
            .expect("packs")
            .as_slice(),
        &[] as &[Value]
    );
}
#[test]
fn old_public_commands_and_unattended_mutations_are_refused() {
    let root = TempDir::new().expect("fixture");
    for command in [
        "review",
        "proposals",
        "runtime",
        "read-approvals",
        "run-approvals",
        "setup",
        "packs",
        "registries",
        "rules",
        "approvals",
        "approve",
        "deny",
        "add",
        "update",
        "remove",
        "search",
        "info",
    ] {
        cargo_bin_cmd!("shield").arg(command).assert().code(2);
    }
    for args in [
        vec!["mcp", "setup", "all"],
        vec!["mcp", "setup", "--agent", "all"],
        vec!["init", "--integration", "skip"],
        vec!["registry", "list", "--global"],
        vec!["__internal", "mcp"],
    ] {
        cargo_bin_cmd!("shield").args(&args).assert().code(2);
    }
    cargo_bin_cmd!("shield")
        .args(["init", "--json", "--project"])
        .arg(root.path())
        .assert()
        .code(1);
    assert!(!root.path().join(".shield").exists());
    root.child("config.json")
        .write_str(r#"{"schema_version":1,"packs":[]}"#)
        .expect("candidate");
    let config = root.path().join("config.json");
    for args in [
        vec![
            "review",
            "init",
            "--project-config",
            config.to_str().expect("path"),
        ],
        vec![
            "runtime",
            "activate",
            "--revision",
            "untrusted",
            "--candidate-digest",
            "untrusted",
        ],
        vec!["read-approvals", "approve", "untrusted"],
        vec!["run-approvals", "approve", "untrusted"],
        vec!["history", "purge"],
    ] {
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(args)
            .arg("--project")
            .arg(root.path())
            .assert()
            .code(2);
    }
    assert!(!root.path().join(".shield").exists());
}

/// Run an interactive command, confirm its review and require success.
fn confirmed(root: &TempDir, args: &[&str]) {
    let result = interactive(root, args, "yes\n");
    assert_eq!(result["code"], 0, "{result}");
}

#[test]
#[cfg(unix)]
fn registry_short_refs_preserve_pins_profiles_and_unchanged_activation() {
    let root = TempDir::new().expect("project");
    let source = TempDir::new().expect("registry");
    let pack = super::policy::candidate(&source);
    let bytes = std::fs::read(pack.path()).expect("pack");
    let digest = Sha256::digest(&bytes)
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("digest formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    for args in [
        vec!["init", "--no-baseline"],
        vec![
            "registry",
            "add",
            "company",
            source.path().to_str().expect("path"),
        ],
        vec![
            "pack",
            "add",
            "company/test@0.1.0",
            "--profile",
            "recommended",
        ],
    ] {
        confirmed(&root, &args);
    }
    confirmed(
        &root,
        &["pack", "add", "company/test", "--profile", "strict"],
    );
    let packs = query(&root, &["pack", "list"]);
    assert_eq!(packs["packs"][0]["pinned"], true);
    assert_eq!(packs["packs"][0]["version"], "0.1.0");
    assert_eq!(packs["packs"][0]["profile"], "strict");
    let before = query(&root, &["status"])["active"].clone();
    confirmed(
        &root,
        &["pack", "add", "company/test", "--profile", "strict"],
    );
    assert_eq!(query(&root, &["status"])["active"], before);
    let other = TempDir::new().expect("other source");
    other.child("pack.json").write_binary(&bytes).expect("pack");
    other
        .child("catalog.json")
        .write_str(&std::fs::read_to_string(source.child("catalog.json").path()).expect("catalog"))
        .expect("catalog");
    for args in [
        vec![
            "registry",
            "add",
            "other",
            other.path().to_str().expect("source"),
        ],
        vec!["registry", "default", "company"],
        vec![
            "pack",
            "add",
            "other/test@0.1.0",
            "--profile",
            "recommended",
        ],
    ] {
        confirmed(&root, &args);
    }
    let before = query(&root, &["status"])["active"].clone();
    // A bare add names the default source, so only company's row is a match;
    // it is already strict, so nothing changes.
    confirmed(&root, &["pack", "add", "test", "--profile", "strict"]);
    assert_eq!(query(&root, &["status"])["active"], before);
    // An installed reference spans every source and reports both instances.
    let result = interactive(&root, &["pack", "update", "test"], "yes\n");
    assert_eq!(result["code"], 1, "{result}");
    let stderr = result["stderr"].as_str().expect("stderr");
    for alternative in ["company/acme/test", "other/acme/test"] {
        assert!(stderr.contains(alternative), "{result}");
    }
    assert_eq!(query(&root, &["status"])["active"], before);
    let output = cargo_bin_cmd!("shield")
        .args(["pack", "show", "test", "--json", "--project"])
        .arg(root.path())
        .env("HOME", root.path())
        .output()
        .expect("pack show");
    let refused: Value = serde_json::from_slice(&output.stdout).expect("json error");
    assert_eq!(
        refused["error"]["alternatives"],
        json!(["company/acme/test", "other/acme/test"]),
        "{refused}"
    );
    assert_eq!(refused["error"]["alternatives_total"], 2, "{refused}");
}

#[test]
#[cfg(unix)]
fn init_from_project_files_writes_a_portable_lock() {
    let locks: Vec<String> = (0..2)
        .map(|_| {
            let root = TempDir::new().expect("project");
            let pack = super::policy::candidate(&root);
            root.child(".shield/packs/test.json")
                .write_binary(&std::fs::read(pack.path()).expect("pack"))
                .expect("project pack");
            root.child(".shield/config.yml")
                .write_str(r#"{"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","profile":"recommended","path":"packs/test.json"}]}"#)
                .expect("config");
            let result = interactive(&root, &["init", "--no-baseline"], "yes\n");
            assert_eq!(result["code"], 0, "{result}");
            let lock = std::fs::read_to_string(root.path().join(".shield/shield.lock"))
                .expect("lock");
            let canonical = root.path().canonicalize().expect("canonical root");
            for path in [root.path(), canonical.as_path()] {
                let path = path.to_str().expect("path");
                assert!(!lock.contains(path), "{lock}");
            }
            assert!(lock.contains("acme/test"), "{lock}");
            lock
        })
        .collect();
    assert_eq!(locks[0], locks[1]);
}

#[test]
#[cfg(unix)]
fn rule_edit_preserves_conditions_and_only_changes_explicit_values() {
    let root = TempDir::new().expect("project");
    let conditions = json!({"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["read"]},"cwd":{"exact":{"root":"absolute","path":root.path()}}});
    root.child(".shield/config.yml")
        .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#)
        .expect("config");
    root.child(".shield/rules.yml").write_str(&json!({"schema_version":1,"rules":[{"id":"read","decision":"allow","reason":"Old reason","conditions":conditions}],"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1","location-v1"]}).to_string()).expect("rules");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let result = interactive(
        &root,
        &["rule", "edit", "read"],
        "\n\n\n\nNew reason\nyes\n",
    );
    assert_eq!(result["code"], 0, "{result}");
    let rules = query(&root, &["rule", "list"]);
    assert_eq!(rules["rules"][0]["conditions"], conditions);
    assert_eq!(rules["rules"][0]["reason"], "New reason");
}

#[test]
#[cfg(unix)]
fn rule_add_and_edit_refuse_rules_naming_the_checkout() {
    let root = TempDir::new().expect("project");
    let checkout = root.path().canonicalize().expect("project path");
    let arguments = json!(["--prefix", checkout, "start"]).to_string();
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let before = query(&root, &["status"])["active"].clone();
    let refused = interactive(
        &root,
        &["rule", "add"],
        &format!("start\nargv\nnpm\n{arguments}\nallow\nStart Acme\n\nyes\n"),
    );
    assert_ne!(refused["code"], 0, "{refused}");
    let stderr = refused["stderr"].as_str().expect("stderr");
    assert!(
        stderr.contains("names a folder on this machine"),
        "{refused}"
    );
    assert_eq!(query(&root, &["status"])["active"], before);
    // A portable rule is accepted; editing it to name the checkout is not.
    let added = interactive(
        &root,
        &["rule", "add"],
        "start\nargv\nnpm\n[\"start\"]\nallow\nStart Acme\n\nyes\n",
    );
    assert_eq!(added["code"], 0, "{added}");
    let before = query(&root, &["status"])["active"].clone();
    let refused = interactive(
        &root,
        &["rule", "edit", "start"],
        &format!("\n\n{arguments}\n\n\nyes\n"),
    );
    assert_ne!(refused["code"], 0, "{refused}");
    assert!(
        refused["stderr"]
            .as_str()
            .expect("stderr")
            .contains("names a folder on this machine"),
        "{refused}"
    );
    assert_eq!(query(&root, &["status"])["active"], before);
}
#[test]
#[cfg(unix)]
fn confirmed_rules_publish_immediately_and_cancellation_preserves_authority() {
    let root = TempDir::new().expect("fixture");
    let cancelled = interactive(&root, &["init"], "no\n");
    assert_eq!(cancelled["code"], 130);
    assert!(!root.path().join(".shield").exists());
    let initialized = interactive(&root, &["init"], "yes\n");
    assert_eq!(initialized["code"], 0, "{initialized}");
    let before = query(&root, &["status"]);
    assert!(before["active"].is_object());
    assert_eq!(before["history"]["limit"], 5000);
    let cancelled = interactive(
        &root,
        &["rule", "add"],
        "status\nargv\ngit\n[\"status\"]\nallow\nRead repository state\n\nno\n",
    );
    assert_eq!(cancelled["code"], 130);
    assert_eq!(query(&root, &["status"])["active"], before["active"]);
    let changed = interactive(
        &root,
        &["rule", "add"],
        "status\nargv\ngit\n[\"status\"]\nallow\nRead repository state\n\nyes\n",
    );
    assert_eq!(changed["code"], 0, "{changed}");
    let after = query(&root, &["status"]);
    assert_ne!(after["active"], before["active"]);
    assert_eq!(after["pending_change"], false);
    assert_eq!(after["repair_needed"], false);
    assert_eq!(
        query(&root, &["history", "list"])["entries"]
            .as_array()
            .expect("history")
            .as_slice(),
        &[] as &[Value]
    );
    root.child(".shield/rules.yml")
        .write_str("manual edit")
        .expect("manual projection edit");
    assert_eq!(query(&root, &["status"])["repair_needed"], true);
    assert_eq!(
        interactive(&root, &["rule", "remove", "status"], "yes\n")["code"],
        1
    );
}

#[cfg(unix)]
pub(super) fn local_dependency_source() -> TempDir {
    let source = TempDir::new().expect("registry");
    let child_file = super::policy::candidate(&source);
    let child: Value =
        serde_json::from_slice(&std::fs::read(child_file.path()).expect("child")).expect("pack");
    let mut parent = child.clone();
    parent["schema_version"] = json!(1);
    parent["id"] = json!("acme/parent");
    parent["capabilities"] = json!(["structured-rules-v1", "dependencies-v1"]);
    parent["rules"] = json!([]);
    parent["dependencies"] = json!([{"pack":"acme/test","requirement":"^0.1"}]);
    let mut incompatible = parent.clone();
    incompatible["version"] = json!("0.2.0");
    incompatible["dependencies"][0]["requirement"] = json!("^2");
    let mut entries = Vec::new();
    for (path, pack) in [
        ("child.yml", child),
        ("parent.yml", parent),
        ("newer.yml", incompatible),
    ] {
        let bytes = pack.to_string();
        source.child(path).write_str(&bytes).expect("artifact");
        let digest =
            Sha256::digest(bytes.as_bytes())
                .iter()
                .fold(String::new(), |mut out, byte| {
                    write!(out, "{byte:02x}").expect("hex");
                    out
                });
        entries
            .push(json!({"id":pack["id"],"version":pack["version"],"path":path,"sha256":digest.to_ascii_uppercase()}));
    }
    source
        .child("catalog.json")
        .write_str(&json!({"schema_version":1,"packs":entries}).to_string())
        .expect("catalog");
    source
}

#[cfg(unix)]
fn fixture_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, byte| {
            write!(out, "{byte:02x}").expect("hex fixture digest");
            out
        })
}

#[cfg(unix)]
fn cross_source_sources() -> (TempDir, TempDir, String) {
    let a = local_dependency_source();
    let b = local_dependency_source();
    let identity = fixture_digest(
        b.path()
            .canonicalize()
            .expect("canonical foreign fixture directory")
            .to_str()
            .expect("UTF-8 fixture directory")
            .as_bytes(),
    );
    let parent_path = a.path().join("parent.yml");
    let mut parent: Value =
        serde_json::from_slice(&std::fs::read(&parent_path).expect("parent artifact"))
            .expect("parent document");
    parent["dependencies"][0]["registry"] = json!(identity);
    let bytes = parent.to_string();
    std::fs::write(parent_path, &bytes).expect("foreign parent artifact");
    let catalog_path = a.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).expect("source catalog"))
            .expect("catalog document");
    catalog["packs"][1]["sha256"] = json!(fixture_digest(bytes.as_bytes()));
    std::fs::write(catalog_path, catalog.to_string()).expect("foreign source catalog");
    (a, b, identity)
}

#[test]
#[cfg(unix)]
fn cross_source_cli_reviews_foreign_updates_and_prunes_offline() {
    let root = TempDir::new().unwrap();
    let (a, b, identity) = cross_source_sources();
    for args in [
        vec!["init", "--no-baseline"],
        vec!["registry", "add", "a", a.path().to_str().unwrap()],
    ] {
        assert_eq!(interactive(&root, &args, "yes\n")["code"], 0);
    }
    let before = query(&root, &["status"])["active"].clone();
    assert_eq!(
        interactive(&root, &["pack", "add", "a/acme/parent"], "\nyes\n")["code"],
        1
    );
    assert_eq!(query(&root, &["status"])["active"], before);
    assert_eq!(
        interactive(
            &root,
            &["registry", "add", "b", b.path().to_str().unwrap()],
            "yes\n"
        )["code"],
        0
    );
    assert_eq!(
        interactive(&root, &["pack", "add", "a/acme/parent"], "\nyes\n")["code"],
        0
    );
    let rows = query(&root, &["pack", "list"])["packs"].clone();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let child = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/test")
        .unwrap();
    assert_eq!(child["source_identity"], identity);
    let mut pack: Value =
        serde_json::from_slice(&std::fs::read(b.path().join("child.yml")).unwrap()).unwrap();
    pack["version"] = json!("0.1.1");
    let bytes = pack.to_string();
    std::fs::write(b.path().join("updated.json"), &bytes).unwrap();
    let catalog_path = b.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["packs"].as_array_mut().unwrap().push(json!({"id":pack["id"],"version":"0.1.1","path":"updated.json","sha256":fixture_digest(bytes.as_bytes())}));
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    let before = query(&root, &["status"])["active"].clone();
    assert_eq!(
        interactive(&root, &["pack", "update", "acme/parent"], "no\n")["code"],
        130
    );
    assert_eq!(query(&root, &["status"])["active"], before);
    assert_eq!(
        interactive(&root, &["pack", "update", "acme/parent"], "yes\n")["code"],
        0
    );
    let updated = query(&root, &["pack", "list"])["packs"].clone();
    let after = updated
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["selector"] == child["selector"])
        .unwrap();
    assert_eq!(after["version"], "0.1.1");
    assert_eq!(after["source_identity"], identity);
    assert_eq!(after["profile"], child["profile"]);
    drop(a);
    drop(b);
    assert_eq!(
        interactive(
            &root,
            &["pack", "remove", child["selector"].as_str().unwrap()],
            "yes\n"
        )["code"],
        1
    );
    assert_eq!(
        interactive(&root, &["pack", "remove", "acme/parent"], "yes\n")["code"],
        0
    );
    assert_eq!(query(&root, &["pack", "list"])["packs"], json!([]));
}

#[test]
#[cfg(unix)]
fn dependency_update_cli_reviews_all_changes_and_preserves_cancelled_activation() {
    let root = TempDir::new().unwrap();
    let source = local_dependency_source();
    for args in [
        vec!["init", "--no-baseline"],
        vec!["registry", "add", "acme", source.path().to_str().unwrap()],
        vec![
            "pack",
            "add",
            "acme/acme/parent",
            "--profile",
            "recommended",
        ],
    ] {
        assert_eq!(interactive(&root, &args, "yes\n")["code"], 0);
    }
    let catalog_path = source.path().join("catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    for template in ["child.yml", "parent.yml"] {
        let mut pack: Value =
            serde_json::from_slice(&std::fs::read(source.path().join(template)).unwrap()).unwrap();
        pack["version"] = json!("0.1.1");
        let path = format!("updated-{template}");
        let bytes = pack.to_string();
        std::fs::write(source.path().join(&path), &bytes).unwrap();
        let digest =
            Sha256::digest(bytes.as_bytes())
                .iter()
                .fold(String::new(), |mut out, byte| {
                    write!(out, "{byte:02x}").unwrap();
                    out
                });
        catalog["packs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":pack["id"],"version":"0.1.1","path":path,"sha256":digest}));
    }
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    let before = query(&root, &["status"])["active"].clone();
    let rows = query(&root, &["pack", "list"])["packs"].clone();
    assert_eq!(interactive(&root, &["pack", "update"], "no\n")["code"], 130);
    assert_eq!(query(&root, &["status"])["active"], before);
    assert_eq!(query(&root, &["pack", "list"])["packs"], rows);
    let updated = interactive(&root, &["pack", "update"], "yes\n");
    assert_eq!(updated["code"], 0, "{updated}");
    let after = query(&root, &["pack", "list"])["packs"].clone();
    for row in after.as_array().unwrap() {
        assert_eq!(row["version"], "0.1.1");
        let previous = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row["id"])
            .unwrap();
        assert_eq!(row["selector"], previous["selector"]);
        assert_eq!(row["profile"], previous["profile"]);
    }
    let active = query(&root, &["status"])["active"].clone();
    assert_eq!(interactive(&root, &["pack", "update"], "yes\n")["code"], 0);
    assert_eq!(query(&root, &["status"])["active"], active);
}

/// The shared lock records the local registry by identity, not its path.
#[cfg(unix)]
fn assert_lock_names_source_by_identity(root: &TempDir, source: &TempDir) {
    let lock = std::fs::read_to_string(root.path().join(".shield/shield.lock")).expect("lock");
    let canonical = source.path().canonicalize().expect("canonical source");
    for path in [source.path(), canonical.as_path()] {
        assert!(!lock.contains(path.to_str().expect("path")), "{lock}");
    }
    let locked: Value = serde_json::from_str(&lock).expect("lock json");
    assert!(locked["packs"].as_array().expect("rows").iter().all(|r| {
        r["source_identity"].is_string()
            && r["source_alias"] == "acme"
            && r["source"]["identity"] == r["source_identity"]
    }));
}

#[test]
#[cfg(unix)]
fn local_dependency_installation_reviews_complete_closure_preserves_profiles_and_works_offline() {
    let root = TempDir::new().expect("project");
    let source = local_dependency_source();
    for args in [
        vec!["init", "--no-baseline"],
        vec![
            "registry",
            "add",
            "acme",
            source.path().to_str().expect("path"),
        ],
    ] {
        let result = interactive(&root, &args, "yes\n");
        assert_eq!(result["code"], 0, "{result}");
    }
    let before = query(&root, &["status"])["active"].clone();
    let cancelled = interactive(&root, &["pack", "add", "acme/acme/parent"], "\nno\n");
    assert_eq!(cancelled["code"], 130, "{cancelled}");
    assert_eq!(query(&root, &["status"])["active"], before);
    let installed = interactive(&root, &["pack", "add", "acme/acme/parent"], "\nyes\n");
    assert_eq!(installed["code"], 0, "{installed}");
    assert_lock_names_source_by_identity(&root, &source);
    let packs = query(&root, &["pack", "list"]);
    let rows = packs["packs"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    let parent = rows
        .iter()
        .find(|r| r["id"] == "acme/parent")
        .expect("parent");
    let child = rows.iter().find(|r| r["id"] == "acme/test").expect("child");
    assert_eq!(
        parent["version"], "0.1.0",
        "backtrack from incompatible newer closure"
    );
    assert_eq!(parent["direct"], true);
    assert_eq!(parent["pinned"], false);
    assert_eq!(child["direct"], false);
    assert_eq!(child["incoming"][0]["requirement"], "^0.1");
    let parent_selector = parent["selector"].as_str().expect("selector");
    let child_selector = child["selector"].as_str().expect("selector");
    let before = query(&root, &["status"])["active"].clone();
    let conflict = interactive(
        &root,
        &["pack", "add", child_selector, "--profile", "loose"],
        "yes\n",
    );
    assert_eq!(conflict["code"], 1, "{conflict}");
    assert!(
        conflict["stderr"]
            .as_str()
            .expect("error")
            .contains("profile conflicts")
    );
    assert_eq!(interactive(&root, &["pack", "update"], "yes\n")["code"], 0);
    let required = interactive(&root, &["pack", "remove", child_selector], "yes\n");
    assert_eq!(required["code"], 1);
    assert!(required["stderr"].as_str().unwrap().contains("acme/parent"));
    assert_eq!(query(&root, &["status"])["active"], before);
    assert_eq!(
        interactive(&root, &["registry", "remove", "acme"], "yes\n")["code"],
        0
    );
    drop(source);
    let changed = interactive(
        &root,
        &["pack", "add", parent_selector, "--profile", "strict"],
        "yes\n",
    );
    assert_eq!(changed["code"], 0, "{changed}");
    let locked: Value = serde_json::from_slice(
        &std::fs::read(root.path().join(".shield/shield.lock")).expect("lock"),
    )
    .expect("json");
    assert!(
        locked["packs"]
            .as_array()
            .expect("rows")
            .iter()
            .all(|r| r["profile"] == "strict")
    );
    assert_eq!(locked["packs"][0]["selector"], parent["selector"]);
    let active = query(&root, &["status"])["active"].clone();
    assert_eq!(
        interactive(&root, &["pack", "remove", parent_selector], "no\n")["code"],
        130
    );
    assert_eq!(query(&root, &["status"])["active"], active);
    assert_eq!(
        interactive(&root, &["pack", "remove", parent_selector], "yes\n")["code"],
        0
    );
    assert_eq!(query(&root, &["pack", "list"])["packs"], json!([]));
}

/// An initialized project with an Ask rule for `/bin/echo`, a `company`
/// registry and its `acme/test` pack installed. Returns the project and the
/// registry folder, which must outlive the project's use of it.
#[cfg(unix)]
fn inspected_project() -> (TempDir, TempDir) {
    let root = TempDir::new().expect("project");
    let rule = json!({"id":"echo","decision":"ask","reason":"Acme echo needs approval","conditions":{"domain":"argv","executable":"/bin/echo","arguments":{"kind":"any"}}});
    root.child(".shield/config.yml")
        .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#)
        .expect("config");
    root.child(".shield/rules.yml")
        .write_str(&json!({"schema_version":1,"rules":[rule],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string())
        .expect("rules");
    let source = TempDir::new().expect("registry");
    let pack = super::policy::candidate(&source);
    let digest = Sha256::digest(std::fs::read(pack.path()).expect("pack"))
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("digest formatting");
            output
        });
    source.child("catalog.json").write_str(&json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":digest}]}).to_string()).expect("catalog");
    for args in [
        vec!["init"],
        vec![
            "registry",
            "add",
            "company",
            source.path().to_str().expect("path"),
        ],
        vec![
            "pack",
            "add",
            "company/test@0.1.0",
            "--profile",
            "recommended",
        ],
    ] {
        let result = interactive(&root, &args, "yes\n");
        assert_eq!(result["code"], 0, "{result}");
    }
    (root, source)
}

/// Every file under `root` with its bytes.
#[cfg(unix)]
fn files(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("folder") {
            let entry = entry.expect("entry");
            if entry.file_type().expect("type").is_dir() {
                folders.push(entry.path());
            } else {
                files.insert(
                    entry.path(),
                    std::fs::read(entry.path()).unwrap_or_default(),
                );
            }
        }
    }
    files
}

#[test]
#[cfg(unix)]
fn every_inspection_result_carries_the_schema_version() {
    let (root, _source) = inspected_project();
    #[cfg_attr(not(target_os = "macos"), expect(unused_mut))]
    let mut inspections = vec![
        vec!["status"],
        vec!["doctor"],
        vec!["policy", "show"],
        vec!["rule", "list"],
        vec!["registry", "list"],
        vec!["registry", "list", "--scope", "user"],
        vec!["pack", "list"],
        vec!["pack", "show", "test"],
        vec!["pack", "search", "company/test"],
        vec!["pack", "info", "company/test"],
        vec!["request", "list"],
        vec!["history", "list"],
        vec!["mcp", "setup", "--agent", "codex", "--dry-run"],
    ];
    // A native Ask launch, which only macOS supports, leaves a request and an entry.
    #[cfg(target_os = "macos")]
    let entry = {
        let run = |extra: &[&str]| {
            cargo_bin_cmd!("shield")
                .args(["__internal", "run", "--project"])
                .arg(root.path())
                .args(extra)
                .args(["--", "/bin/echo", "Acme"])
                .env("HOME", root.path())
                .output()
                .expect("native run")
        };
        assert_eq!(run(&[]).status.code(), Some(3));
        let request = query(&root, &["request", "list"])["requests"][0]["request_id"]
            .as_str()
            .expect("request")
            .to_owned();
        // Approving consumes the request once its retry runs, so inspect it first.
        let pending = query(&root, &["request", "show", &request]);
        assert_eq!(pending["schema_version"], 1, "{pending}");
        let approved = interactive(&root, &["request", "approve", &request], "yes\n");
        assert_eq!(approved["code"], 0, "{approved}");
        assert!(run(&["--approval-request", &request]).status.success());
        query(&root, &["history", "list"])["entries"][0]["entry_id"]
            .as_str()
            .expect("history entry")
            .to_owned()
    };
    #[cfg(target_os = "macos")]
    inspections.push(vec!["history", "show", entry.as_str()]);
    for args in &inspections {
        let result = query(&root, args);
        assert_eq!(result["schema_version"], 1, "{args:?}: {result}");
    }
}

#[test]
#[cfg(unix)]
fn every_mutation_refuses_json_before_preparing_anything() {
    let (root, source) = inspected_project();
    let location = source.path().to_str().expect("path");
    let before = files(root.path());
    for args in [
        vec!["pack", "add", "company/test@0.1.0", "--profile", "strict"],
        vec!["pack", "update"],
        vec!["pack", "update", "test"],
        vec!["pack", "remove", "test"],
        vec!["pack", "configure", "test", "--override", "mutation=deny"],
        vec!["pack", "configure", "acme/unknown"],
        vec!["rule", "add"],
        vec!["rule", "edit", "echo"],
        vec!["rule", "remove", "echo"],
        vec!["registry", "add", "other", location],
        vec!["registry", "remove", "company"],
        vec!["registry", "default", "company"],
        vec!["registry", "add", "other", location, "--scope", "user"],
        vec!["registry", "remove", "company", "--scope", "user"],
        vec!["registry", "default", "company", "--scope", "user"],
        vec!["protection", "set"],
        vec!["policy", "review"],
        vec!["mcp", "setup", "--agent", "claude"],
        vec!["request", "approve", "request-1"],
        vec!["request", "deny", "request-1"],
    ] {
        let output = cargo_bin_cmd!("shield")
            .args(&args)
            .args(["--json", "--no-debug-log", "--project"])
            .arg(root.path())
            .env("HOME", root.path())
            .output()
            .expect("refusal");
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let refused: Value = serde_json::from_slice(&output.stdout).expect("JSON error");
        assert_eq!(
            refused["error"]["message"],
            "invalid review input or state: JSON is read-only; run this command interactively",
            "{args:?}: {refused}"
        );
        assert!(files(root.path()) == before, "{args:?} changed state");
    }
}

#[test]
#[cfg(unix)]
fn escape_in_protection_goes_back_with_answers_kept() {
    let root = TempDir::new().expect("project");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let before = query(&root, &["status"])["path_access"].clone();
    // Escape on the first question cancels.
    let set = ["protection", "set"];
    assert_eq!(interactive(&root, &set, "back\n")["code"], 130);
    assert_eq!(query(&root, &["status"])["path_access"], before);
    // Allow reading, go back from the next question, and keep that answer.
    let changed = interactive(&root, &set, "2\nback\n\n\n\nyes\n");
    assert_eq!(changed["code"], 0, "{changed}");
    let after = query(&root, &["status"])["path_access"].clone();
    assert_eq!(after["agent_settings_read"], "allow", "{after}");
    assert_eq!(
        after["agent_settings_change"], before["agent_settings_change"],
        "{after}"
    );
    assert_eq!(
        after["shield_config_change"], before["shield_config_change"],
        "{after}"
    );
}

#[test]
#[cfg(unix)]
fn escape_in_init_and_setup_goes_back_with_answers_kept() {
    let root = TempDir::new().expect("project");
    // The agent page is init's first step without a baseline question.
    assert_eq!(guided(&root, &["init"], "back\n")["code"], 130);
    assert!(!root.path().join(".shield/review/state.json").exists());
    // Back from the permissions keeps the marked agent.
    let result = guided(&root, &["init"], "claude\nback\n\n\nyes\n");
    assert_eq!(result["code"], 0, "{result}");
    let agents = |root: &TempDir| -> Vec<String> {
        let saved: Value = serde_json::from_slice(
            &std::fs::read(root.path().join(".shield/review/state.json")).expect("state"),
        )
        .expect("state JSON");
        saved["operator"]["integration"]["agents"]
            .as_object()
            .map(|agents| agents.keys().cloned().collect())
            .unwrap_or_default()
    };
    assert_eq!(agents(&root), ["claude"]);
    let other = TempDir::new().expect("project");
    assert_eq!(interactive(&other, &["init"], "yes\n")["code"], 0);
    // Back from the permissions returns to the agent page with the agent kept.
    let result = interactive(&other, &["mcp", "setup"], "claude\nback\n\n\nyes\n");
    assert_eq!(result["code"], 0, "{result}");
    assert_eq!(agents(&other), ["claude"]);
}

#[test]
#[cfg(unix)]
fn mcp_setup_dry_run_reviews_without_writing() {
    let root = TempDir::new().expect("project");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    let state = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&state).expect("state");
    let review = query(&root, &["mcp", "setup", "--agent", "codex", "--dry-run"]);
    assert_eq!(review["dry_run"], true, "{review}");
    assert_eq!(review["operation"], "set up agent integrations", "{review}");
    assert_eq!(std::fs::read(&state).expect("state"), before);
    assert!(!root.path().join(".codex").exists());
    assert!(!root.path().join(".mcp.json").exists());
}

#[test]
#[cfg(unix)]
fn escape_in_policy_history_and_rule_add_goes_back_with_answers_kept() {
    let root = TempDir::new().expect("project");
    assert_eq!(interactive(&root, &["init"], "yes\n")["code"], 0);
    // Back from recording returns to the policy menu at the history row.
    let review = ["policy", "review"];
    let inspected = interactive(&root, &review, "history\nback\n1\n");
    assert_eq!(inspected["code"], 0, "{inspected}");
    assert_eq!(query(&root, &["status"])["history"]["enabled"], true);
    // Back from clearing returns to recording with the earlier answers kept.
    let changed = interactive(&root, &review, "history\nno\n7\nback\n\n\n\nyes\n");
    assert_eq!(changed["code"], 0, "{changed}");
    let history = query(&root, &["status"])["history"].clone();
    assert_eq!(history["enabled"], false, "{history}");
    assert_eq!(history["limit"], 7, "{history}");
    // Back from the decision returns to matching, which keeps its choice.
    let added = interactive(
        &root,
        &["rule", "add"],
        "status\nargv\ngit\n[\"status\"]\nback\n\ngit\n[\"log\"]\nallow\nRead the log\n\nyes\n",
    );
    assert_eq!(added["code"], 0, "{added}");
    let rules = query(&root, &["rule", "list"]).to_string();
    assert!(rules.contains(r#""values":["log"]"#), "{rules}");
    assert!(!rules.contains(r#""values":["status"]"#), "{rules}");
}
