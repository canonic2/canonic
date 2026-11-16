//! In-session recovery through the native hooks and the MCP tool together:
//! a refused settings change asks the person and the identical retry runs
//! once, while fixed protected state never offers the route.
use super::super::{find, translate_event};
use crate::{agents::Event, configuration::Bundle, review};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn fixture() -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::create_dir_all(root.path().join(".codex")).unwrap();
    fs::write(root.path().join(".codex/config.toml"), "# Acme\n").unwrap();
    fs::create_dir_all(root.path().join(".claude")).unwrap();
    fs::write(root.path().join(".claude/settings.json"), "{}").unwrap();
    enroll(root.path());
    (root, context)
}

/// Record Claude Code and Codex integrations, whose MCP connections can ask
/// the person.
fn enroll(root: &Path) {
    review::operator::integrations::configure_fixture(
        root,
        review::integrations::IntegrationChoice::Agents(
            review::integrations::IntegrationChoice::bits(&["claude", "codex"]).unwrap(),
        ),
        review::integrations::AskFallback::Block,
    );
}

/// A policy that denies `acme` through rule `acme-config` and asks for `acme-push`.
fn policy_fixture() -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let any = json!({"kind":"any"});
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[
            {"id":"acme-config","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":any}},
            {"id":"acme-push","decision":"ask","conditions":{"domain":"argv","executable":"acme-push","arguments":any}}
        ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    enroll(root.path());
    (root, context)
}

#[test]
fn an_unenrolled_agent_is_routed_to_setup_instead_of_request_approve() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"acme-config","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]})
    .to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let root = Path::new(&context.project_root);
    review::operator::integrations::configure_fixture(
        root,
        review::integrations::IntegrationChoice::agent("codex"),
        review::integrations::AskFallback::Block,
    );

    // Claude Code has no recorded integration here; Codex does.
    let refused = shell(root, "claude", "acme config set claude");
    assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(offered_id(&refused).is_none(), "{refused}");
    assert!(
        !refused.to_string().contains("request_approve"),
        "{refused}"
    );
    assert!(!reason(&refused).contains(super::OFFER), "{refused}");
    assert!(
        reason(&refused).contains("approval connection is not registered"),
        "{refused}"
    );
    assert!(
        agent_context(&refused).contains("shield mcp setup"),
        "{refused}"
    );
    // Nothing is stored that no connection could approve.
    let listed = review::mcp::call_with_confirmation(
        &mcp(root, "codex"),
        &review::mcp::Connection::new("acme-connection".into()),
        "request_list",
        &json!({}),
        &|| Ok(()),
        false,
    );
    assert_eq!(listed["data"]["requests"], json!([]), "{listed}");

    let offered = shell(root, "codex", "acme config set codex");
    assert!(offered_id(&offered).is_some(), "{offered}");
}

#[test]
fn invalid_policy_without_registration_names_policy_repair_before_setup() {
    for agent in ["claude", "codex"] {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
        review::mcp::initialize_fixture(root.path(), &bundle);
        let path = root.path().join(".shield/config.yml");
        fs::write(&path, "packs: [\n").unwrap();
        let refused = shell(root.path(), agent, "pwd");
        assert_eq!(
            refused["hookSpecificOutput"]["permissionDecision"], "deny",
            "{refused}"
        );
        assert!(offered_id(&refused).is_none(), "{refused}");
        assert!(
            agent_context(&refused).contains("user must first repair the policy files"),
            "{refused}"
        );
        assert!(
            agent_context(&refused).contains("Setup also needs valid policy"),
            "{refused}"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "packs: [\n");
    }
}

#[test]
fn an_older_hub_definition_restores_approval_in_a_fresh_checkout() {
    let (_folder, context) = policy_fixture();
    let root = Path::new(&context.project_root);
    fs::create_dir_all(root.join(".hub/mcp")).unwrap();
    fs::create_dir_all(root.join(".hub/local/mcp")).unwrap();
    fs::write(root.join(".hub/config.yml"), "schema_version: 1\n").unwrap();
    let shared = include_str!("../../../../fixtures/hub-shield-definition.yml");
    fs::write(root.join(".hub/mcp/shield.yml"), shared).unwrap();
    let local = json!({"schema_version":1,"connection":{
        "transport":"stdio","command":std::env::current_exe().unwrap(),
        "args":["mcp","serve","--project",".","--agent","{agent}"]}})
    .to_string();
    fs::write(root.join(".hub/local/mcp/shield.yml"), &local).unwrap();
    review::update_operator_fixture(&context, |operator| {
        operator["integration"] = Value::Null;
        // Simulate a negative result cached by the previous implementation.
        let old = json!([
            "hub",
            shared,
            Some(&local),
            std::env::current_exe().unwrap()
        ])
        .to_string();
        operator["box_adoption_checked"] = json!(crate::policy::fingerprint(old.as_bytes()));
    })
    .unwrap();

    let command = "acme config set codex";
    let refused = shell(root, "codex", command);
    let id = offered_id(&refused).expect("a fresh checkout keeps Shield's approval route");
    assert!(review::cli::registered(&review::open(root).unwrap(), "codex").unwrap());
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    let retry = shell(root, "codex", command);
    assert!(agent_context(&retry).contains("allowed once"), "{retry}");
    assert_eq!(
        fs::read_to_string(root.join(".hub/mcp/shield.yml")).unwrap(),
        shared
    );
    assert_eq!(
        fs::read_to_string(root.join(".hub/local/mcp/shield.yml")).unwrap(),
        local
    );
}

#[test]
fn concurrent_native_hooks_capture_fresh_sessions_without_spurious_refusals() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    let rules = crate::policy::rules::document(&[
        json!({"id":"acme-read","decision":"allow","conditions":{
            "domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}),
    ])
    .unwrap()
    .to_string();
    fs::write(root.join(".shield/rules.yml"), rules).unwrap();
    fs::write(
        root.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    // Every batch races to capture a new conversation; no command is executed.
    for batch in 0..8 {
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            let calls: Vec<_> = (0..4)
                .map(|call| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        hook(
                            root,
                            "codex",
                            Event::Before,
                            &json!({
                        "hook_event_name":"PreToolUse","tool_name":"Bash",
                        "tool_use_id":format!("acme-{batch}-{call}"),
                        "session_id":format!("acme-fresh-{batch}"),"cwd":root,
                        "tool_input":{"command":"acme read"}}),
                        )
                    })
                })
                .collect();
            for call in calls {
                let reply = call.join().unwrap();
                assert!(
                    reply["hookSpecificOutput"]
                        .get("permissionDecision")
                        .is_none(),
                    "{reply}"
                );
            }
        });
    }
}

fn mcp(root: &Path, agent: &str) -> review::Context {
    let mut context = review::open(root).unwrap();
    context.surface = "mcp".into();
    context.agent = Some(agent.into());
    context
}

fn reason(value: &Value) -> &str {
    value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default()
}

fn hook(root: &Path, agent: &str, event: Event, host: &Value) -> Value {
    translate_event(
        root,
        find(agent).unwrap(),
        &serde_json::to_vec(host).unwrap(),
        event,
        &crate::diagnostics::Log::disabled(),
    )
}

fn shell(root: &Path, agent: &str, command: &str) -> Value {
    hook(
        root,
        agent,
        Event::Before,
        &json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-call",
            "session_id":"acme-session","cwd":root,"tool_input":{"command":command}}),
    )
}

fn agent_context(value: &Value) -> &str {
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
}

/// The request ID the refusal tells the agent to pass to `request_approve`.
fn offered_id(value: &Value) -> Option<String> {
    let context = agent_context(value);
    let start = context.find("request_id hook-request-")? + "request_id ".len();
    let id = &context[start..];
    Some(id[..id.find(';')?].to_owned())
}

fn request_approve(root: &Path, agent: &str, id: &str, confirmed: bool) -> Value {
    let mut context = review::open(root).unwrap();
    context.surface = "mcp".into();
    context.agent = Some(agent.into());
    review::mcp::call_with_confirmation(
        &context,
        &review::mcp::Connection::new("acme-connection".into()),
        "request_approve",
        &json!({"request_id":id}),
        &|| Ok(()),
        confirmed,
    )
}

#[test]
fn approved_native_retry_history_keeps_policy_and_records_the_actual_allowance() {
    for (agent, history_enabled) in [
        ("claude", true),
        ("codex", true),
        ("claude", false),
        ("codex", false),
    ] {
        let (_root, context) = policy_fixture();
        let root = Path::new(&context.project_root);
        if !history_enabled {
            let settings =
                review::history_settings(&context, Some(false), None, true, &|| Ok(())).unwrap();
            assert_eq!(settings["enabled"], false, "{settings}");
        }
        let command = "acme config set example";
        let id = offered_id(&shell(root, agent, command)).unwrap();
        assert_eq!(
            request_approve(root, agent, &id, true)["data"]["status"],
            "approved"
        );
        let retry = shell(root, agent, command);
        assert!(agent_context(&retry).contains("allowed once"), "{retry}");
        if agent == "codex" {
            let followup = hook(
                root,
                agent,
                Event::Permission,
                &json!({"hook_event_name":"PermissionRequest","session_id":"acme-session","cwd":root,
                    "tool_name":"Bash","tool_input":{"command":command,"workdir":root}}),
            );
            assert_eq!(
                followup["hookSpecificOutput"]["decision"]["behavior"], "allow",
                "{followup}"
            );
        }
        // A third identical call is a separate blocked attempt, not covered by
        // the consumed approval or joined by command text.
        assert_eq!(
            shell(root, agent, command)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        let history = review::mcp::call_with_confirmation(
            &mcp(root, agent),
            &review::mcp::Connection::new("native-approval-history".into()),
            "history_list",
            &json!({"limit":100}),
            &|| Ok(()),
            false,
        );
        let entries = history["data"]["entries"].as_array().unwrap();
        if !history_enabled {
            assert!(entries.is_empty(), "{history}");
            continue;
        }
        let allowed: Vec<_> = entries
            .iter()
            .filter(|entry| entry["approved"] == true)
            .collect();
        assert_eq!(
            allowed.len(),
            if agent == "codex" { 2 } else { 1 },
            "{history}"
        );
        for entry in allowed {
            assert_eq!(entry["disposition"], "allowed", "{entry}");
            assert_eq!(entry["denied"], false, "{entry}");
            assert_eq!(entry["code"], "one-time-approved", "{entry}");
            assert_eq!(entry["approval"]["request_id"], id, "{entry}");
            assert!(entry["approval"]["approval_id"].is_string(), "{entry}");
            assert_eq!(entry["evaluation"]["decision"], "deny", "{entry}");
            assert_eq!(entry["outcome"], "unobserved", "{entry}");
        }
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry["denied"] == true)
                .count(),
            2,
            "{history}"
        );
    }
}

#[test]
fn codex_settings_change_recovers_in_the_session_once() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let command = "git checkout -- .codex/config.toml";

    let refused = shell(root, "codex", command);
    assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
    let id = offered_id(&refused).expect("the refusal names request_approve");
    assert!(
        agent_context(&refused).contains("repeat this exact call once"),
        "{refused}"
    );
    assert!(
        refused["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("The agent can ask you to allow it once."),
        "{refused}"
    );
    assert!(
        !refused.to_string().contains("shield protection set"),
        "{refused}"
    );

    // The agent cannot grant it without the person's answer.
    let unanswered = request_approve(root, "codex", &id, false);
    assert_eq!(unanswered["error"]["code"], "operator_review_required");
    assert_eq!(
        shell(root, "codex", command)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );

    let approved = request_approve(root, "codex", &id, true);
    assert_eq!(approved["data"]["status"], "approved", "{approved}");

    let retried = shell(root, "codex", command);
    assert!(
        retried["hookSpecificOutput"]
            .get("permissionDecision")
            .is_none(),
        "{retried}"
    );
    assert!(
        agent_context(&retried).contains("allowed once"),
        "{retried}"
    );

    // Codex's escalated retry reaches PermissionRequest for the same command.
    let followup = hook(
        root,
        "codex",
        Event::Permission,
        &json!({"hook_event_name":"PermissionRequest","session_id":"acme-session","cwd":root,
            "tool_name":"Bash","tool_input":{"command":command,"workdir":root,
            "description":"Discard the Acme change"}}),
    );
    assert_eq!(
        followup["hookSpecificOutput"]["decision"]["behavior"], "allow",
        "{followup}"
    );

    // The grant is spent: the next identical call asks again.
    let again = shell(root, "codex", command);
    assert_eq!(again["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_ne!(offered_id(&again).unwrap(), id);
}

/// Gate keeps Git metadata in an entry folder and sets core.worktree. The
/// command is evaluated by the hook here; no approved merge is executed.
#[test]
fn a_gate_fast_forward_that_changes_policy_can_be_approved_once() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    let entry = root.join(".gate/repos/private");
    fs::create_dir_all(&entry).unwrap();
    let metadata = entry.join(".git");
    let git = |arguments: &[&str]| {
        let output = crate::test_support::git()
            .current_dir(root)
            .args(["--git-dir", metadata.to_str().unwrap()])
            .args(["-c", "user.name=Acme", "-c", "user.email=acme@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "core.worktree", root.to_str().unwrap()]);
    git(&["add", "--", ".shield/config.yml"]);
    git(&["commit", "-q", "-m", "Acme base"]);
    let base = git(&["rev-parse", "HEAD"]);
    let config = fs::read_to_string(root.join(".shield/config.yml")).unwrap();
    fs::write(
        root.join(".shield/config.yml"),
        format!("{config}# Acme update\n"),
    )
    .unwrap();
    git(&["commit", "-q", "-am", "Acme policy update"]);
    let incoming = git(&["rev-parse", "HEAD"]);
    git(&["reset", "--hard", &base]);

    let command = format!(
        "git -C .gate/repos/private merge --ff-only {}",
        &incoming[..7]
    );
    let refused = shell(root, "codex", &command);
    let id = offered_id(&refused).expect("Git policy changes offer Shield's approval form");
    assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
    let question =
        review::hook_approval::confirmation(&mcp(root, "codex"), &json!({"request_id":id}))
            .unwrap();
    assert!(question.contains(&command), "{question}");
    assert!(
        question.contains("replace Shield's policy files"),
        "{question}"
    );
    assert!(
        question.contains("turn off Shield's protection"),
        "{question}"
    );
    assert!(
        !question.contains("does not change your policy"),
        "{question}"
    );
    assert_eq!(
        request_approve(root, "codex", &id, false)["error"]["code"],
        "operator_review_required"
    );
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );

    // A changed target cannot spend the grant for the reviewed command.
    let other = shell(
        root,
        "codex",
        "git -C .gate/repos/private merge --ff-only acme-other",
    );
    assert_eq!(other["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_ne!(offered_id(&other).unwrap(), id);
    let retry = shell(root, "codex", &command);
    assert!(agent_context(&retry).contains("allowed once"), "{retry}");
    let again = shell(root, "codex", &command);
    assert_eq!(again["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_ne!(offered_id(&again).unwrap(), id);
    assert_eq!(git(&["rev-parse", "HEAD"]), base);
}

#[test]
fn git_policy_approval_does_not_open_direct_writes_or_fixed_state() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    for command in [
        "git checkout HEAD -- .shield/config.yml",
        "git merge --ff-only acme; touch .shield/state/acme",
        "git merge --ff-only acme; rm -rf .shield",
    ] {
        let refused = shell(root, "codex", command);
        assert_eq!(
            refused["hookSpecificOutput"]["permissionDecision"], "deny",
            "{refused}"
        );
        assert!(offered_id(&refused).is_none(), "{refused}");
    }
    // Unknown Git writes have the same explicit warning and confirmation;
    // approval does not turn their uncertainty into an ordinary policy Allow.
    let command = "git stash pop";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    assert!(agent_context(&shell(root, "codex", command)).contains("allowed once"));
}

fn restoration_git(root: &Path, arguments: &[&str]) -> std::process::Output {
    crate::test_support::git()
        .current_dir(root)
        .args([
            "-c",
            "user.name=Acme",
            "-c",
            "user.email=acme@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn git_restoration_reviews_both_effects_and_allows_only_the_identical_retry() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    assert!(restoration_git(root, &["init", "-q"]).status.success());
    for agent in ["codex", "claude"] {
        let command = "git checkout --ours -- .shield/config.yml .codex/config.toml";
        let refused = shell(root, agent, command);
        let id = offered_id(&refused)
            .expect("a modeled policy/settings restoration offers exact approval");
        let question =
            review::hook_approval::confirmation(&mcp(root, agent), &json!({"request_id":id}))
                .unwrap();
        assert!(question.contains("Changing agent settings"), "{question}");
        assert!(
            question.contains("replace Shield's policy files"),
            "{question}"
        );
        assert!(question.contains("Git target worktrees:"), "{question}");
        assert!(question.contains(root.to_str().unwrap()), "{question}");
        assert_eq!(
            request_approve(root, agent, &id, false)["error"]["code"],
            "operator_review_required"
        );
        assert_eq!(
            request_approve(root, agent, &id, true)["data"]["status"],
            "approved"
        );
        let changed = shell(
            root,
            agent,
            "git checkout --theirs -- .shield/config.yml .codex/config.toml",
        );
        assert_eq!(
            changed["hookSpecificOutput"]["permissionDecision"], "deny",
            "{changed}"
        );
        assert_ne!(offered_id(&changed).unwrap(), id);
        let retry = shell(root, agent, command);
        assert!(agent_context(&retry).contains("allowed once"), "{retry}");
        let again = shell(root, agent, command);
        assert_eq!(
            again["hookSpecificOutput"]["permissionDecision"], "deny",
            "{again}"
        );
        assert_ne!(offered_id(&again).unwrap(), id);
    }
}

#[test]
fn invalid_policy_recovery_records_reviews_and_consumes_without_policy_writes() {
    for agent in ["claude", "codex"] {
        for file in ["config.yml", "rules.yml", "shield.lock"] {
            let (_folder, context) = fixture();
            let root = Path::new(&context.project_root);
            assert!(restoration_git(root, &["init", "-q"]).status.success());
            let broken = "<<<<<<< main\ninvalid policy\n=======\nother policy\n>>>>>>> topic\n";
            let path = root.join(".shield").join(file);
            if file == "rules.yml" {
                fs::write(
                    root.join(".shield/config.yml"),
                    "schema_version: 1\npacks: []\nrules: rules.yml\n",
                )
                .unwrap();
            }
            fs::write(&path, broken).unwrap();
            let command = format!("git checkout --ours -- .shield/{file} .codex/config.toml");
            let refused = shell(root, agent, &command);
            let id = offered_id(&refused).unwrap_or_else(|| panic!("{file}: {refused}"));
            assert!(
                reason(&refused).contains("policy files in .shield/ are invalid"),
                "{refused}"
            );
            let question =
                review::hook_approval::confirmation(&mcp(root, agent), &json!({"request_id":id}))
                    .unwrap();
            for warning in [
                "current policy files are invalid",
                "Changing agent settings",
                "replace Shield's policy files",
                "Git target worktrees:",
            ] {
                assert!(question.contains(warning), "{question}");
            }
            assert_eq!(
                request_approve(root, agent, &id, false)["error"]["code"],
                "operator_review_required"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), broken);
            assert_eq!(
                offered_id(&shell(root, agent, &command)).as_deref(),
                Some(id.as_str())
            );
            assert_eq!(
                request_approve(root, agent, &id, true)["data"]["status"],
                "approved"
            );
            // Current broken bytes may change between review and retry; no
            // earlier policy becomes authority and Git content is not frozen.
            fs::write(&path, format!("{broken}# changed after confirmation\n")).unwrap();
            let retry = shell(root, agent, &command);
            assert!(agent_context(&retry).contains("allowed once"), "{retry}");
            assert_ne!(
                offered_id(&shell(root, agent, &command)).as_deref(),
                Some(id.as_str())
            );
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                format!("{broken}# changed after confirmation\n")
            );
        }
    }
}

#[test]
fn invalid_policy_recovery_trace_explains_work() {
    use clap::Parser as _;
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::write(root.join(".shield/config.yml"), "packs: [\n").unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "--project",
        root.to_str().unwrap(),
        "--json",
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log
        .request("native_hook", "native_hook", None, None)
        .unwrap();
    let started = std::time::Instant::now();
    {
        let _scope = trace.enter();
        let refused = shell(root, "codex", "pwd");
        assert!(offered_id(&refused).is_some(), "{refused}");
    }
    trace.finish("blocked");
    log.flush();
    assert_eq!(
        trace.recorded_count("integration.binary_hashes"),
        0,
        "invalid policy must stop source-dependent maintenance before executable hashing"
    );
    println!(
        "invalid policy hook: {:?}, rebinds {}, binary hashes {}, policy captures {}\n{}",
        started.elapsed(),
        trace.recorded_count("integration.rebind_attempts"),
        trace.recorded_count("integration.binary_hashes"),
        trace.recorded_count("policy.captures"),
        fs::read_to_string(log.path().unwrap()).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn invalid_policy_recovery_rechecks_fixed_targets_before_spending_an_approval() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    assert!(restoration_git(root, &["init", "-q"]).status.success());
    fs::write(root.join(".shield/config.yml"), "packs: [\n").unwrap();
    let command = "git checkout --ours -- .shield/config.yml .codex/config.toml";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    let settings = root.join(".codex/config.toml");
    let original = fs::read(&settings).unwrap();
    let moved = settings.clone();
    let state = root.join(".shield/state/state.json");
    let boundary = crate::enforcement::protocol::before_native_authority(move || {
        fs::remove_file(&moved).unwrap();
        std::os::unix::fs::symlink(state, moved).unwrap();
    });
    let refused = shell(root, "codex", command);
    assert_eq!(
        refused["hookSpecificOutput"]["permissionDecision"], "deny",
        "{refused}"
    );
    assert!(offered_id(&refused).is_none(), "{refused}");
    drop(boundary);
    fs::remove_file(&settings).unwrap();
    fs::write(&settings, original).unwrap();
    let retry = shell(root, "codex", command);
    assert!(agent_context(&retry).contains("allowed once"), "{retry}");
}

#[test]
fn invalid_policy_recovery_keeps_pending_and_approved_requests_actionable() {
    for before_approval in [false, true] {
        let (_folder, context) = fixture();
        let root = Path::new(&context.project_root);
        assert!(restoration_git(root, &["init", "-q"]).status.success());
        let command = "git checkout --ours -- .shield/config.yml .codex/config.toml";
        let id = offered_id(&shell(root, "codex", command)).unwrap();
        if before_approval {
            assert_eq!(
                request_approve(root, "codex", &id, true)["data"]["status"],
                "approved"
            );
        }
        fs::write(root.join(".shield/config.yml"), "packs: [\n").unwrap();
        if !before_approval {
            assert_eq!(
                offered_id(&shell(root, "codex", command)).as_deref(),
                Some(id.as_str())
            );
            let approved = request_approve(root, "codex", &id, true);
            assert_eq!(approved["data"]["status"], "approved", "{approved}");
        }
        let retry = shell(root, "codex", command);
        assert!(agent_context(&retry).contains("allowed once"), "{retry}");
        let followup = hook(
            root,
            "codex",
            Event::Permission,
            &json!({"hook_event_name":"PermissionRequest","session_id":"acme-session","cwd":root,
                "tool_name":"Bash","tool_input":{"command":command,"workdir":root}}),
        );
        assert_eq!(
            followup["hookSpecificOutput"]["decision"]["behavior"], "allow",
            "{followup}"
        );
        assert_eq!(
            shell(root, "codex", command)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
}

#[test]
fn invalid_policy_recovery_preserves_fixed_guards_and_never_uses_old_allow() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    assert!(restoration_git(root, &["init", "-q"]).status.success());
    fs::write(root.join(".shield/config.yml"), "packs: [\n").unwrap();
    for agent in ["claude", "codex"] {
        for command in [
            "cat .shield/state/state.json",
            "cat .shield/cache/compiled.bin",
            "printf bad > .shield/config.yml",
            "shield protection set",
            "git checkout --ours -- .shield",
            "git checkout --ours -- .shield/.gitignore",
            "git checkout --ours -- .shield/state/state.json",
            "git checkout --ours -- .shield/config.yml; printf bad > .shield/rules.yml",
            "eval acme",
        ] {
            let refused = shell(root, agent, command);
            assert_eq!(
                refused["hookSpecificOutput"]["permissionDecision"], "deny",
                "{command}: {refused}"
            );
            assert!(offered_id(&refused).is_none(), "{command}: {refused}");
        }
        let refused = shell(root, agent, "pwd");
        let id = offered_id(&refused).unwrap_or_else(|| panic!("{refused}"));
        assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
        let other = mcp(root, if agent == "claude" { "codex" } else { "claude" });
        assert!(review::hook_approval::approve(&other, &json!({"request_id":id}), true).is_err());
        assert_eq!(
            request_approve(root, agent, &id, true)["data"]["status"],
            "approved"
        );
        let other_session = hook(
            root,
            agent,
            Event::Before,
            &json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"other-call",
                "session_id":"other-session","cwd":root,"tool_input":{"command":"pwd"}}),
        );
        assert_eq!(
            other_session["hookSpecificOutput"]["permissionDecision"], "deny",
            "{other_session}"
        );
        let retry = shell(root, agent, "pwd");
        assert!(agent_context(&retry).contains("allowed once"), "{retry}");
    }
}

#[test]
fn invalid_policy_recovery_repairs_and_aborts_a_real_active_worktree_rebase() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    let checked = |arguments: &[&str]| {
        let out = restoration_git(root, arguments);
        assert!(out.status.success(), "{arguments:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    checked(&["init", "-q"]);
    checked(&["add", ".shield/config.yml"]);
    checked(&["commit", "-qm", "base"]);
    checked(&["switch", "-qc", "topic"]);
    let config = root.join(".shield/config.yml");
    fs::write(&config, "schema_version: 1\npacks: []\n# Acme topic\n").unwrap();
    fs::write(root.join("feature.txt"), "Acme feature\n").unwrap();
    checked(&["add", ".shield/config.yml", "feature.txt"]);
    checked(&["commit", "-qm", "feature"]);
    let original = checked(&["rev-parse", "HEAD"]);
    checked(&["switch", "-q", "main"]);
    let main = "schema_version: 1\npacks: []\n# Acme main\n";
    fs::write(&config, main).unwrap();
    checked(&["add", ".shield/config.yml"]);
    checked(&["commit", "-qm", "main"]);
    checked(&["switch", "-q", "topic"]);
    assert!(!restoration_git(root, &["rebase", "main"]).status.success());
    assert!(fs::read_to_string(&config).unwrap().contains("<<<<<<<"));
    let command = "git checkout --ours -- .shield/config.yml";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    let approved = request_approve(root, "codex", &id, true);
    assert_eq!(approved["data"]["status"], "approved", "{approved}");
    let retry = shell(root, "codex", command);
    assert!(agent_context(&retry).contains("allowed once"), "{retry}");
    checked(&["checkout", "--ours", "--", ".shield/config.yml"]);
    assert_eq!(fs::read_to_string(&config).unwrap(), main);
    assert!(
        crate::runtime::Project::open(root)
            .unwrap()
            .snapshot()
            .is_ok()
    );
    checked(&["add", ".shield/config.yml"]);
    checked(&["-c", "core.editor=true", "rebase", "--continue"]);
    assert_eq!(
        fs::read_to_string(root.join("feature.txt")).unwrap(),
        "Acme feature\n"
    );
    assert!(checked(&["status", "--porcelain", "--untracked-files=no"]).is_empty());
    checked(&["reset", "--hard", &original]);
    assert!(!restoration_git(root, &["rebase", "main"]).status.success());
    let command = "git rebase --abort";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    let retry = shell(root, "codex", command);
    assert!(agent_context(&retry).contains("allowed once"), "{retry}");
    checked(&["rebase", "--abort"]);
    assert_eq!(checked(&["rev-parse", "HEAD"]), original);
    assert!(
        crate::runtime::Project::open(root)
            .unwrap()
            .snapshot()
            .is_ok()
    );
}

#[test]
fn git_restoration_keeps_direct_writes_unknown_forms_and_fixed_neighbors_closed() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    assert!(restoration_git(root, &["init", "-q"]).status.success());
    for command in [
        "git checkout --ours -- .shield/state/state.json .codex/config.toml",
        "git checkout --ours -- .codex/config.toml .shield/config.yml .shield/state/state.json",
        "git checkout --ours -- .shield/.gitignore .shield/config.yml",
        "git checkout --ours -- .shield/cache/acme",
        "git checkout --ours -- .shield",
        "git checkout --ours -- .",
        "git checkout --ours -- '.shield/*'",
        "git checkout --ours -- .shield/config.yml > .shield/config.yml",
        "git checkout --ours -- .shield/config.yml; printf x > .shield/config.yml",
        "git checkout --ours -- .shield/config.yml; shield protection set",
        "git restore --future-option -- .shield/config.yml",
        "git restore --pathspec-from-file=paths -- .shield/config.yml",
        "git -c alias.restore='!acme' restore -- .shield/config.yml",
        "cat acme.txt > .shield/config.yml",
    ] {
        for agent in ["claude", "codex"] {
            let refused = shell(root, agent, command);
            assert_eq!(
                refused["hookSpecificOutput"]["permissionDecision"], "deny",
                "{command}: {refused}"
            );
            assert!(offered_id(&refused).is_none(), "{command}: {refused}");
        }
    }
}

#[test]
fn git_restoration_supports_revision_index_forms_decline_and_open_settings() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    assert!(restoration_git(root, &["init", "-q"]).status.success());
    for command in [
        "git checkout HEAD -- .shield/config.yml",
        "git restore --source=HEAD -- .shield/config.yml",
        "git restore --source HEAD --staged --worktree -- .shield/config.yml",
        "git checkout-index --stage=2 -- .shield/config.yml",
    ] {
        let refused = shell(root, "codex", command);
        let id = offered_id(&refused).expect("a complete literal restoration can be reviewed");
        let denied = review::mcp::call_with_confirmation(
            &mcp(root, "codex"),
            &review::mcp::Connection::new("acme-decline".into()),
            "request_deny",
            &json!({"request_id":id}),
            &|| Ok(()),
            true,
        );
        assert_eq!(denied["data"]["status"], "revoked", "{denied}");
        let retry = shell(root, "codex", command);
        assert_eq!(
            retry["hookSpecificOutput"]["permissionDecision"], "deny",
            "{retry}"
        );
        assert_ne!(offered_id(&retry).unwrap(), id);
    }
    review::path_access::configure_fixture(
        root,
        review::path_access::PathAccess {
            agent_settings_change: review::path_access::Change::Allow,
            ..Default::default()
        },
    );
    let command = "git restore --source=HEAD -- .shield/config.yml .codex/config.toml";
    let refused = shell(root, "codex", command);
    let id = offered_id(&refused).unwrap();
    let question =
        review::hook_approval::confirmation(&mcp(root, "codex"), &json!({"request_id":id}))
            .unwrap();
    assert!(question.contains("Changing agent settings"), "{question}");
    assert!(
        question.contains("replace Shield's policy files"),
        "{question}"
    );
    let claude = shell(root, "claude", command);
    assert_eq!(
        claude["hookSpecificOutput"]["permissionDecision"], "ask",
        "{claude}"
    );
    assert!(
        reason(&claude).contains("protected agent settings"),
        "{claude}"
    );
    assert!(reason(&claude).contains("Target worktrees:"), "{claude}");
}

#[test]
#[cfg(unix)]
fn git_restoration_cannot_spend_a_grant_after_the_target_link_moves() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    for name in ["first", "second"] {
        let target = root.join(name);
        fs::create_dir_all(target.join(".shield")).unwrap();
        fs::write(
            target.join(".shield/config.yml"),
            "schema_version: 1\npacks: []\n",
        )
        .unwrap();
        assert!(restoration_git(&target, &["init", "-q"]).status.success());
    }
    let link = root.join("target");
    std::os::unix::fs::symlink(root.join("first"), &link).unwrap();
    let command = "git -C target checkout --ours -- .shield/config.yml";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(root.join("second"), &link).unwrap();
    let moved = shell(root, "codex", command);
    assert_eq!(
        moved["hookSpecificOutput"]["permissionDecision"], "deny",
        "{moved}"
    );
    assert_ne!(offered_id(&moved).unwrap(), id);
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(root.join("first"), &link).unwrap();
    assert!(agent_context(&shell(root, "codex", command)).contains("allowed once"));
}

#[test]
fn git_restoration_without_a_known_root_cannot_open_policy_even_for_index_only_changes() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    for command in [
        "git checkout --ours -- .shield/config.yml",
        "git restore --staged -- .shield/config.yml",
    ] {
        let refused = shell(root, "codex", command);
        assert_eq!(
            refused["hookSpecificOutput"]["permissionDecision"], "deny",
            "{refused}"
        );
        assert!(offered_id(&refused).is_none(), "{refused}");
    }
}

#[test]
#[cfg(unix)]
fn git_restoration_binds_each_call_when_a_compound_commands_target_set_stays_the_same() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    for name in ["first", "second"] {
        let target = root.join(name);
        fs::create_dir_all(target.join(".shield")).unwrap();
        fs::write(
            target.join(".shield/config.yml"),
            "schema_version: 1\npacks: []\n",
        )
        .unwrap();
        assert!(restoration_git(&target, &["init", "-q"]).status.success());
    }
    let a = root.join("a");
    let b = root.join("b");
    std::os::unix::fs::symlink(root.join("first"), &a).unwrap();
    std::os::unix::fs::symlink(root.join("second"), &b).unwrap();
    let command = "git -C a checkout --ours -- .shield/config.yml; git -C b checkout --theirs -- .shield/config.yml";
    let id = offered_id(&shell(root, "codex", command)).unwrap();
    let form = review::hook_approval::confirmation(&mcp(root, "codex"), &json!({"request_id":id}))
        .unwrap();
    assert!(form.contains("Git restoration calls"), "{form}");
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    fs::remove_file(&a).unwrap();
    fs::remove_file(&b).unwrap();
    std::os::unix::fs::symlink(root.join("second"), &a).unwrap();
    std::os::unix::fs::symlink(root.join("first"), &b).unwrap();
    let swapped = shell(root, "codex", command);
    assert_eq!(
        swapped["hookSpecificOutput"]["permissionDecision"], "deny",
        "{swapped}"
    );
    assert_ne!(offered_id(&swapped).unwrap(), id);
}

/// Review through the hook/MCP consumer, then execute only the accepted fixture command.
fn reviewed_restoration_git(root: &Path, arguments: &[&str]) -> std::process::Output {
    let command = format!("git -C target {}", arguments.join(" "));
    let refused = shell(root, "codex", &command);
    let id = offered_id(&refused).unwrap_or_else(|| panic!("{command}: {refused}"));
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    let retry = shell(root, "codex", &command);
    assert!(
        agent_context(&retry).contains("allowed once"),
        "{command}: {retry}"
    );
    restoration_git(&root.join("target"), arguments)
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the divergent-branch fixture, reviewed continuations and abort assertions together"
)]
fn git_restoration_rebase_lifecycle_resolves_continues_and_aborts_in_another_worktree() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    let target = root.join("target");
    fs::create_dir_all(target.join(".shield")).unwrap();
    fs::create_dir_all(target.join(".codex")).unwrap();
    let checked = |args: &[&str]| {
        let out = restoration_git(&target, args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let write_side = |side: &str| {
        fs::write(
            target.join(".shield/config.yml"),
            format!("schema_version: 1\npacks: []\n# Acme {side}\n"),
        )
        .unwrap();
        fs::write(
            target.join(".codex/config.toml"),
            format!("# Acme {side}\n"),
        )
        .unwrap();
    };
    checked(&["init", "-q"]);
    write_side("base");
    fs::write(target.join("keep.txt"), "Acme unchanged\n").unwrap();
    checked(&[
        "add",
        "--",
        ".shield/config.yml",
        ".codex/config.toml",
        "keep.txt",
    ]);
    checked(&["commit", "-q", "-m", "Acme base"]);
    checked(&["switch", "-q", "-c", "topic"]);
    for side in ["topic-one", "topic-two"] {
        write_side(side);
        fs::write(target.join(side), "Acme feature\n").unwrap();
        checked(&["add", "--", "."]);
        checked(&["commit", "-q", "-m", side]);
    }
    let original = checked(&["rev-parse", "HEAD"]);
    checked(&["switch", "-q", "main"]);
    write_side("main");
    checked(&["commit", "-q", "-am", "Acme main"]);
    checked(&["switch", "-q", "topic"]);
    let parent_policy = fs::read(root.join(".shield/config.yml")).unwrap();
    assert!(
        !reviewed_restoration_git(root, &["rebase", "main"])
            .status
            .success()
    );
    for pass in 0..2 {
        assert!(
            fs::read_to_string(target.join(".shield/config.yml"))
                .unwrap()
                .contains("<<<<<<<")
        );
        assert!(
            reviewed_restoration_git(
                root,
                &[
                    "checkout",
                    "--ours",
                    "--",
                    ":/.shield/config.yml",
                    ":/.codex/config.toml"
                ]
            )
            .status
            .success()
        );
        assert!(
            reviewed_restoration_git(
                root,
                &["add", "--", ":/.shield/config.yml", ":/.codex/config.toml"]
            )
            .status
            .success()
        );
        let continued =
            reviewed_restoration_git(root, &["-c", "core.editor=true", "rebase", "--continue"]);
        assert_eq!(continued.status.success(), pass == 1, "{continued:?}");
    }
    assert!(target.join("topic-one").is_file() && target.join("topic-two").is_file());
    assert!(
        fs::read_to_string(target.join(".shield/config.yml"))
            .unwrap()
            .contains("Acme main")
    );
    assert!(checked(&["status", "--porcelain"]).is_empty());
    checked(&["reset", "--hard", &original]);
    assert!(
        !reviewed_restoration_git(root, &["rebase", "main"])
            .status
            .success()
    );
    assert!(
        reviewed_restoration_git(root, &["rebase", "--abort"])
            .status
            .success()
    );
    assert_eq!(checked(&["rev-parse", "HEAD"]), original);
    assert!(checked(&["status", "--porcelain"]).is_empty());
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "Acme unchanged\n"
    );
    assert_eq!(
        fs::read(root.join(".shield/config.yml")).unwrap(),
        parent_policy
    );
}

#[test]
fn a_git_policy_change_beside_a_settings_write_keeps_both_warnings() {
    let (_folder, context) = fixture();
    let root = Path::new(&context.project_root);
    let command = "git stash pop; cat acme.txt > .claude/settings.json";
    let refused = shell(root, "codex", command);
    let id = offered_id(&refused).expect("both effects can be opened once");
    let question =
        review::hook_approval::confirmation(&mcp(root, "codex"), &json!({"request_id":id}))
            .unwrap();
    assert!(question.contains("Changing agent settings"), "{question}");
    assert!(question.contains("not a frozen Git revision"), "{question}");
}

#[test]
fn a_git_policy_deny_override_also_warns_about_replacing_policy() {
    let folder = assert_fs::TempDir::new().unwrap();
    let rules = crate::policy::rules::document(&[
        json!({"id":"acme-git-deny","decision":"deny","conditions":{
            "domain":"argv","executable":"git","arguments":{"kind":"any"}}}),
    ])
    .unwrap()
    .to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(folder.path(), &bundle);
    let root = Path::new(&context.project_root);
    enroll(root);
    let id = offered_id(&shell(root, "codex", "git stash pop")).unwrap();
    let question =
        review::hook_approval::confirmation(&mcp(root, "codex"), &json!({"request_id":id}))
            .unwrap();
    assert!(question.contains("acme-git-deny"), "{question}");
    assert!(
        question.contains("turn off Shield's protection"),
        "{question}"
    );
}

#[test]
fn claude_settings_change_recovers_in_the_session_once() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let command = "cat acme.txt > .claude/settings.json";
    let refused = shell(root, "claude", command);
    assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
    let id = offered_id(&refused).expect("the refusal names request_approve");
    // Another agent's connection cannot answer for this one.
    assert!(!request_approve(root, "codex", &id, true)["error"].is_null());
    assert_eq!(
        request_approve(root, "claude", &id, true)["data"]["status"],
        "approved"
    );
    let retried = shell(root, "claude", command);
    assert_eq!(
        retried["hookSpecificOutput"]["permissionDecision"], "allow",
        "{retried}"
    );
}

#[test]
fn grouping_guidance_keeps_the_single_request_route_and_pending_id() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    shell(root, "codex", "git checkout -- .codex/config.toml");
    let command = "git checkout -- .claude/settings.json";
    let refused = shell(root, "codex", command);
    let id = offered_id(&refused).unwrap();
    let guidance = agent_context(&refused);
    assert!(
        guidance.contains("only requests still needed"),
        "{guidance}"
    );
    assert!(guidance.contains("up to eight"), "{guidance}");
    assert!(guidance.contains("12,000-byte"), "{guidance}");
    assert!(guidance.contains("retry_arguments"), "{guidance}");
    assert!(guidance.contains("one request_id"), "{guidance}");
    // An identical refusal keeps its ID; the hint does not require collecting more.
    assert_eq!(offered_id(&shell(root, "codex", command)), Some(id.clone()));
    assert_eq!(
        request_approve(root, "codex", &id, true)["data"]["status"],
        "approved"
    );
    assert!(agent_context(&shell(root, "codex", command)).contains("allowed once"));
}

#[test]
fn a_retry_with_changed_execution_options_needs_its_own_approval() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let call = |sandbox_off: bool, description: &str| {
        hook(
            root,
            "claude",
            Event::Before,
            &json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-call",
                "session_id":"acme-session","cwd":root,"tool_input":{
                    "command":"cat acme.txt > .claude/settings.json",
                    "description":description,"dangerouslyDisableSandbox":sandbox_off}}),
        )
    };
    let id = offered_id(&call(false, "Copy the Acme settings")).unwrap();
    let question =
        review::hook_approval::confirmation(&mcp(root, "claude"), &json!({"request_id":id}))
            .unwrap();
    assert!(
        question.contains("\"dangerouslyDisableSandbox\": false"),
        "{question}"
    );
    assert_eq!(
        request_approve(root, "claude", &id, true)["data"]["status"],
        "approved"
    );

    // Turning off the sandbox is a different request; the grant stays unused.
    let changed = call(true, "Copy the Acme settings");
    assert_eq!(changed["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_ne!(offered_id(&changed).unwrap(), id);
    // Only the explanatory description may differ on the identical retry.
    let retried = call(false, "Retry the Acme copy");
    assert_eq!(
        retried["hookSpecificOutput"]["permissionDecision"], "allow",
        "{retried}"
    );
}

#[test]
fn a_policy_deny_names_its_rule_and_recovers_once() {
    let (_root, context) = policy_fixture();
    let root = Path::new(&context.project_root);
    for agent in ["codex", "claude"] {
        let command = format!("acme config set {agent}");
        let refused = shell(root, agent, &command);
        assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(reason(&refused).ends_with(super::OFFER), "{refused}");
        let id = offered_id(&refused).expect("a policy deny can be approved once");
        let question =
            review::hook_approval::confirmation(&mcp(root, agent), &json!({"request_id":id}))
                .unwrap();
        assert!(question.contains(&format!("    {command}")), "{question}");
        assert!(
            question.contains("overrides the rule that denies it: acme-config."),
            "{question}"
        );
        assert_eq!(
            request_approve(root, agent, &id, true)["data"]["status"],
            "approved"
        );
        let retried = shell(root, agent, &command);
        assert_ne!(
            retried["hookSpecificOutput"]["permissionDecision"], "deny",
            "{retried}"
        );
    }
}

#[test]
fn codex_offers_its_unpromptable_ask_only_when_the_form_shows_it_whole() {
    let (_root, context) = policy_fixture();
    let root = Path::new(&context.project_root);
    let asked = shell(root, "codex", "acme-push origin");
    assert!(offered_id(&asked).is_some(), "{asked}");
    assert_eq!(
        reason(&asked),
        format!(
            "Shield: approval required. Codex cannot request approval here.{}",
            super::OFFER
        )
    );

    // A request the form cannot show whole is never offered.
    let long = vec!["a".repeat(400); 33].join(" ");
    for command in [format!("acme-push {long}"), format!("acme {long}")] {
        let refused = shell(root, "codex", &command);
        assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(offered_id(&refused).is_none(), "{refused}");
        assert!(!reason(&refused).contains(super::OFFER), "{refused}");
        assert!(
            agent_context(&refused).contains("split it into smaller calls"),
            "{refused}"
        );
    }
}

#[test]
fn a_patch_that_also_reaches_review_state_is_not_offered() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for state in [".shield/state", ".shield/review"] {
        let patch = format!(
            "*** Begin Patch\n*** Update File: .codex/config.toml\n@@\n-# Acme\n+# Acme changed\n*** Add File: {state}/acme.json\n+{{}}\n*** End Patch"
        );
        let value = hook(
            root,
            "codex",
            Event::Before,
            &json!({"hook_event_name":"PreToolUse","tool_name":"apply_patch","tool_use_id":"acme-call",
                "session_id":"acme-session","cwd":root,"tool_input":{"command":patch}}),
        );
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(offered_id(&value).is_none(), "{value}");
        assert!(!reason(&value).contains(super::OFFER), "{value}");
    }
}

#[test]
fn fixed_protected_state_never_offers_in_session_approval() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["codex", "claude"] {
        for command in [
            "cat acme.txt > .shield/state/acme.json",
            "cat acme.txt > .shield/review/acme.json",
            "rm -rf .shield",
            "cat acme.txt > .shield/.gitignore",
            // A settings change cannot carry runtime state with it.
            "sed -i 's/a/b/' .codex/config.toml; touch .shield/state/acme",
            "sed -i 's/a/b/' .codex/config.toml; touch .shield/review/acme",
            "shield protection set",
        ] {
            let value = shell(root, agent, command);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{agent} {command}: {value}"
            );
            assert!(offered_id(&value).is_none(), "{agent} {command}: {value}");
            assert!(
                !value.to_string().contains("request_approve"),
                "{agent} {command}: {value}"
            );
        }
    }
}
