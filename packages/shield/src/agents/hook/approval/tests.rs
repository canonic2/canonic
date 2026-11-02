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
    (root, context)
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
    (root, context)
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
    let patch = "*** Begin Patch\n*** Update File: .codex/config.toml\n@@\n-# Acme\n+# Acme changed\n*** Add File: .shield/review/acme.json\n+{}\n*** End Patch";
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

#[test]
fn fixed_protected_state_never_offers_in_session_approval() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["codex", "claude"] {
        for command in [
            "cat acme.txt > .shield/review/acme.json",
            "rm -rf .shield",
            "cat acme.txt > .shield/.gitignore",
            // A settings change cannot carry review state with it.
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
