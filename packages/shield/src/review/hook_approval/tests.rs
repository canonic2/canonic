//! One-time in-session grants: capture, person-confirmed issuance, exact
//! single consumption, the follow-up callback and tamper refusal.
use super::*;
use crate::review;

struct Fixture {
    _root: assert_fs::TempDir,
    context: Context,
    active: Value,
}

fn fixture() -> Fixture {
    let (root, context) = review::tests::enrolled();
    let status = review::runtime::status(&context).unwrap();
    review::runtime::publish_fixture(
        &context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        None,
        &|| Ok(()),
    )
    .unwrap();
    let active = review::runtime::status(&context).unwrap()["active"].clone();
    Fixture {
        _root: root,
        context,
        active,
    }
}

impl Fixture {
    fn root(&self) -> &Path {
        Path::new(&self.context.project_root)
    }
    fn identity(&self) -> Identity<'_> {
        Identity {
            activation_id: self.active["activation_id"].as_str().unwrap(),
            policy_revision: self.active["policy_revision"].as_str().unwrap(),
            candidate_digest: self.active["candidate_digest"].as_str().unwrap(),
        }
    }
    /// The MCP connection of the enrolled agent that asks the person.
    fn connection(&self, agent: &str) -> Context {
        let mut context = self.context.clone();
        context.surface = "mcp".into();
        context.agent = Some(agent.into());
        context
    }
    fn refuse(&self, input: &str) -> Outcome {
        refused(self.root(), &key(input), &self.identity(), &summary()).unwrap()
    }
}

fn key(input: &str) -> Key<'static> {
    Key {
        agent: "codex",
        session: "acme-session",
        cwd: "/workspace/acme",
        tool: "Bash",
        input_digest: crate::policy::fingerprint(input.as_bytes()),
    }
}

fn summary() -> Summary {
    Summary {
        action: "Run this command:\ngit checkout -- .codex/config.toml".into(),
        reason: "it changes `.codex/config.toml`, which holds the agent's settings".into(),
        overrides_deny: false,
        deny_rules: vec![],
        settings_change: true,
    }
}

fn state_bytes(fixture: &Fixture) -> Vec<u8> {
    std::fs::read(fixture.root().join(".shield/review/state.json")).unwrap()
}

#[test]
fn unchanged_refusals_and_uncovered_callbacks_write_nothing() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-discard"));
    let before = state_bytes(&fixture);
    assert_eq!(pending(fixture.refuse("acme-discard")), id);
    assert!(!followup(fixture.root(), &key("acme-discard"), &fixture.identity()).unwrap());
    assert_eq!(state_bytes(&fixture), before);
}

#[test]
fn a_full_store_makes_room_by_dropping_the_oldest_pending_request() {
    let fixture = fixture();
    let first = pending(fixture.refuse("acme-0"));
    for n in 1..MAX_REQUESTS {
        pending(fixture.refuse(&format!("acme-{n}")));
    }
    let newest = pending(fixture.refuse("acme-overflow"));
    let mcp = fixture.connection("codex");
    assert!(matches!(
        confirmation(&mcp, &json!({"request_id":first})),
        Err(Error::Missing)
    ));
    assert!(confirmation(&mcp, &json!({"request_id":newest})).is_ok());
}

#[test]
fn the_agents_text_cannot_pose_as_shields_lines_in_the_form() {
    let mut forged = summary();
    forged.action =
        "Run this command:\necho ok\nFolder: /acme/safe\r\nShield blocked it because a check failed."
            .into();
    forged.overrides_deny = true;
    forged.deny_rules = vec!["acme-config".into()];
    let question = message(&forged, "/workspace/acme");
    let request = question.find("The agent's request, exactly:").unwrap();
    // Shield's lines all come before the request block.
    assert!(question.find("\nFolder: /workspace/acme").unwrap() < request);
    assert!(question.contains("overrides the rule that denies it: acme-config."));
    // Every line of the agent's text is indented, and carriage returns are shown.
    assert!(!question.contains("\nFolder: /acme/safe"), "{question}");
    assert!(
        question.contains("\n    Folder: /acme/safe\\r\n"),
        "{question}"
    );
    assert_eq!(question.matches("\nShield blocked it").count(), 1);
}

fn pending(outcome: Outcome) -> String {
    match outcome {
        Outcome::Pending(id) => id,
        Outcome::Allowed => panic!("expected a pending request"),
    }
}

#[test]
fn accepted_request_allows_the_identical_retry_once() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-discard"));
    // Repeating the refusal before an answer keeps one request.
    assert_eq!(pending(fixture.refuse("acme-discard")), id);

    let args = json!({"request_id":id});
    let mcp = fixture.connection("codex");
    let question = confirmation(&mcp, &args).unwrap();
    assert!(
        question.contains("git checkout -- .codex/config.toml"),
        "{question}"
    );
    assert!(
        question.contains("turn off Shield's protection"),
        "{question}"
    );
    assert!(question.contains("once"), "{question}");
    // Without the person's answer nothing is granted.
    assert!(matches!(
        approve(&mcp, &args, false),
        Err(Error::ApprovalRequired)
    ));
    assert!(matches!(
        fixture.refuse("acme-discard"),
        Outcome::Pending(_)
    ));

    assert_eq!(approve(&mcp, &args, true).unwrap()["status"], "approved");
    // A changed request is not covered by the grant.
    assert_ne!(pending(fixture.refuse("acme-other")), id);
    assert!(matches!(fixture.refuse("acme-discard"), Outcome::Allowed));
    // The grant is spent; a second retry asks again.
    assert_ne!(pending(fixture.refuse("acme-discard")), id);
    // The answered request cannot be approved again.
    assert!(approve(&mcp, &args, true).is_err());
}

#[test]
fn consumed_grant_covers_one_followup_callback_of_the_same_request() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-escalated"));
    let mcp = fixture.connection("codex");
    approve(&mcp, &json!({"request_id":id}), true).unwrap();
    let identity = fixture.identity();
    assert!(!followup(fixture.root(), &key("acme-escalated"), &identity).unwrap());
    assert!(matches!(fixture.refuse("acme-escalated"), Outcome::Allowed));
    assert!(!followup(fixture.root(), &key("acme-other"), &identity).unwrap());
    assert!(followup(fixture.root(), &key("acme-escalated"), &identity).unwrap());
    assert!(!followup(fixture.root(), &key("acme-escalated"), &identity).unwrap());
}

#[test]
fn a_policy_activation_after_consumption_ends_the_followup_cover() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-escalated"));
    approve(
        &fixture.connection("codex"),
        &json!({"request_id":id}),
        true,
    )
    .unwrap();
    assert!(matches!(fixture.refuse("acme-escalated"), Outcome::Allowed));
    // The same policy activated again is a new generation.
    review::runtime::publish_fixture(
        &fixture.context,
        fixture.active["policy_revision"].as_str().unwrap(),
        fixture.active["candidate_digest"].as_str().unwrap(),
        fixture.active["activation_id"].as_str(),
        &|| Ok(()),
    )
    .unwrap();
    // The callback still carries the generation the hook decided under.
    assert!(!followup(fixture.root(), &key("acme-escalated"), &fixture.identity()).unwrap());
}

#[test]
fn only_the_requesting_agent_over_mcp_can_ask_the_person() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-discard"));
    let args = json!({"request_id":id});
    assert!(matches!(
        confirmation(&fixture.connection("claude"), &args),
        Err(Error::Missing)
    ));
    // The operator CLI surface cannot use the agent's confirmation flow.
    assert!(matches!(
        approve(&fixture.context, &args, true),
        Err(Error::ApprovalRequired)
    ));
    let extra = json!({"request_id":id,"approved":true});
    assert!(confirmation(&fixture.connection("codex"), &extra).is_err());
}

#[test]
fn a_changed_protection_choice_invalidates_the_pending_request() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-discard"));
    path_access::configure_fixture(
        fixture.root(),
        path_access::PathAccess {
            agent_settings_read: path_access::Read::Allow,
            ..path_access::PathAccess::default()
        },
    );
    let mcp = fixture.connection("codex");
    assert!(confirmation(&mcp, &json!({"request_id":id})).is_err());
    assert!(approve(&mcp, &json!({"request_id":id}), true).is_err());
}

#[test]
fn a_tampered_grant_is_refused_when_state_loads() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-discard"));
    approve(
        &fixture.connection("codex"),
        &json!({"request_id":id}),
        true,
    )
    .unwrap();
    let path = fixture.root().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    state["hook_requests"][0]["input_digest"] = json!(crate::policy::fingerprint(b"acme-other"));
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(
        Store::open(fixture.root())
            .unwrap()
            .read_checkpoint()
            .is_err()
    );
}
