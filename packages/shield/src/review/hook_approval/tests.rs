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
    fn refuse_with(&self, session: &str, input: &str, summary: &Summary) -> Outcome {
        let mut key = key(input);
        key.session = session;
        refused(self.root(), &key, &self.identity(), summary).unwrap()
    }
}

fn key(input: &str) -> Key<'_> {
    Key {
        agent: "codex",
        session: "acme-session",
        cwd: "/workspace/acme",
        tool: "Bash",
        input_digest: crate::policy::fingerprint(input.as_bytes()),
    }
}

/// A refused branch deletion that overrides a deny rule.
fn deny_summary(branch: &str) -> Summary {
    Summary {
        action: format!("Run this command:\ngit branch -D {branch}"),
        reason: "the active policy denies it".into(),
        overrides_deny: true,
        deny_rules: vec!["acme-branch-delete".into()],
        settings_change: false,
    }
}

#[test]
fn a_group_accepted_in_one_form_allows_each_identical_retry_once() {
    let fixture = fixture();
    let settings = fixture.refuse("acme-discard");
    assert!(matches!(settings, Outcome::Pending(_, false)));
    let branch = fixture.refuse_with("acme-session", "acme-branch", &deny_summary("acme/task"));
    // The second refusal hints at grouping without naming the first.
    assert!(matches!(branch, Outcome::Pending(_, true)));
    let ids = [pending(settings), pending(branch)];
    let args = json!({"request_ids":ids});
    let mcp = fixture.connection("codex");

    let question = confirmation(&mcp, &args).unwrap();
    assert!(
        question.contains("Accepting creates 2 separate one-time permissions. 1 of them overrides a rule that denies it. 1 of them changes agent settings."),
        "{question}"
    );
    let first = question.find("Request 1 of 2").unwrap();
    let second = question.find("Request 2 of 2").unwrap();
    assert!(first < question.find(".codex/config.toml").unwrap());
    assert!(second < question.find("git branch -D acme/task").unwrap());
    assert!(question.contains("overrides the rule that denies it: acme-branch-delete."));

    let result = approve(&mcp, &args, true).unwrap();
    let grants = result["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0]["request_id"], ids[0].as_str());
    assert_eq!(grants[1]["request_id"], ids[1].as_str());
    let state = Store::open(fixture.root())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    let issued: Vec<_> = state
        .authorization_events
        .iter()
        .filter(|event| event["kind"] == "hook_grant_issued")
        .collect();
    assert_eq!(issued.len(), 2);
    assert!(
        issued
            .iter()
            .all(|e| e["links"]["approval_id"] == result["approval_id"])
    );

    // A changed request is not covered; retries run in any order, once each.
    assert!(matches!(fixture.refuse("acme-other"), Outcome::Pending(..)));
    let branch = || fixture.refuse_with("acme-session", "acme-branch", &deny_summary("acme/task"));
    assert!(matches!(branch(), Outcome::Allowed));
    assert!(matches!(fixture.refuse("acme-discard"), Outcome::Allowed));
    assert!(matches!(branch(), Outcome::Pending(..)));
    assert!(matches!(
        fixture.refuse("acme-discard"),
        Outcome::Pending(..)
    ));
    // The answered group cannot be approved again.
    assert!(matches!(
        approve(&mcp, &args, true),
        Err(Error::Grouped {
            reason: "it was already answered",
            ..
        })
    ));
}

#[test]
fn a_group_with_one_unapprovable_request_refuses_whole() {
    let fixture = fixture();
    let ours = pending(fixture.refuse("acme-discard"));
    let theirs = pending(fixture.refuse_with(
        "acme-other-session",
        "acme-branch",
        &deny_summary("acme/task"),
    ));
    let mcp = fixture.connection("codex");

    let mixed = json!({"request_ids":[ours, theirs]});
    match approve(&mcp, &mixed, true) {
        Err(Error::Grouped {
            request,
            reason,
            recovery,
        }) => {
            assert_eq!(request, theirs);
            assert_eq!(reason, "it was refused in another session");
            assert_eq!(recovery, WITHOUT_IT);
        }
        other => panic!("expected a group refusal, got {:?}", other.map(|_| ())),
    }
    // An unknown ID is named by position, never echoed.
    match confirmation(&mcp, &json!({"request_ids":[ours, "acme\nforged"]})) {
        Err(Error::Grouped { request, .. }) => assert_eq!(request, "request_ids[1]"),
        other => panic!("expected a group refusal, got {:?}", other.map(|_| ())),
    }
    // Nothing was issued: our request is still pending.
    assert_eq!(pending(fixture.refuse("acme-discard")), ours);

    // A list of one gets the single request's form and refusals, but a
    // list-shaped result.
    let one = json!({"request_ids":[ours]});
    let question = confirmation(&mcp, &one).unwrap();
    assert!(
        question.starts_with("Allow the agent to do this once?"),
        "{question}"
    );
    assert_eq!(
        approve(&mcp, &one, true).unwrap()["grants"][0]["request_id"],
        ours.as_str()
    );
    assert!(matches!(
        confirmation(&mcp, &one),
        Err(Error::Invalid(reason)) if reason.contains("repeat the identical call once if it was approved")
    ));

    for args in [
        json!({"request_id":ours,"request_ids":[ours]}),
        json!({"request_ids":[]}),
        json!({"request_ids":[ours, ours]}),
        json!({"request_ids":(0..9).map(|n| format!("r-{n}")).collect::<Vec<_>>()}),
        json!({}),
    ] {
        assert!(
            matches!(confirmation(&mcp, &args), Err(Error::Invalid(_))),
            "{args}"
        );
    }
}

#[test]
fn a_request_answered_while_the_form_is_open_makes_accept_issue_nothing() {
    let fixture = fixture();
    let first = pending(fixture.refuse("acme-discard"));
    let second =
        pending(fixture.refuse_with("acme-session", "acme-branch", &deny_summary("acme/task")));
    let mcp = fixture.connection("codex");
    let group = json!({"request_ids":[first, second]});
    confirmation(&mcp, &group).unwrap();
    // While the group form is open, the first request is answered alone.
    approve(&mcp, &json!({"request_id":first}), true).unwrap();
    assert!(matches!(
        approve(&mcp, &group, true),
        Err(Error::Grouped {
            reason: "it was already answered",
            ..
        })
    ));
    // The second request stayed pending.
    assert_eq!(
        pending(fixture.refuse_with("acme-session", "acme-branch", &deny_summary("acme/task"))),
        second
    );

    // A policy activation while the form is open invalidates the whole group.
    let third = pending(fixture.refuse("acme-third"));
    let group = json!({"request_ids":[second, third]});
    confirmation(&mcp, &group).unwrap();
    review::runtime::publish_fixture(
        &fixture.context,
        fixture.active["policy_revision"].as_str().unwrap(),
        fixture.active["candidate_digest"].as_str().unwrap(),
        fixture.active["activation_id"].as_str(),
        &|| Ok(()),
    )
    .unwrap();
    assert!(matches!(
        approve(&mcp, &group, true),
        Err(Error::Grouped { .. })
    ));
}

#[test]
fn the_grouping_hint_ignores_requests_a_policy_change_invalidated() {
    let fixture = fixture();
    pending(fixture.refuse("acme-old"));
    review::runtime::publish_fixture(
        &fixture.context,
        fixture.active["policy_revision"].as_str().unwrap(),
        fixture.active["candidate_digest"].as_str().unwrap(),
        fixture.active["activation_id"].as_str(),
        &|| Ok(()),
    )
    .unwrap();
    let active = review::runtime::status(&fixture.context).unwrap()["active"].clone();
    let identity = Identity {
        activation_id: active["activation_id"].as_str().unwrap(),
        policy_revision: active["policy_revision"].as_str().unwrap(),
        candidate_digest: active["candidate_digest"].as_str().unwrap(),
    };
    let refuse = |input| refused(fixture.root(), &key(input), &identity, &summary()).unwrap();
    // The request from before the activation could no longer join a group.
    assert!(matches!(refuse("acme-new"), Outcome::Pending(_, false)));
    assert!(matches!(refuse("acme-newer"), Outcome::Pending(_, true)));
}

#[test]
fn a_group_too_long_to_show_whole_is_refused() {
    // UTF-8 bytes, rather than character count, decide whether even two fit.
    for (character, expected_prefix) in [("a", 3), ("é", 1)] {
        let fixture = fixture();
        let mut long = summary();
        long.action = format!("Run this command:\necho {}", character.repeat(3_500));
        let ids: Vec<String> = (0..8)
            .map(|n| pending(fixture.refuse_with("acme-session", &format!("acme-{n}"), &long)))
            .collect();
        let mcp = fixture.connection("codex");
        let args = json!({"request_ids":ids});
        let before = state_bytes(&fixture);
        let Err(Error::ApprovalFormTooLong { retry, remaining }) = confirmation(&mcp, &args) else {
            panic!("expected a fitting prefix and remaining IDs");
        };
        assert_eq!(retry, ids[..expected_prefix]);
        assert_eq!(remaining, ids[expected_prefix..]);
        assert!(matches!(
            approve(&mcp, &args, true),
            Err(Error::ApprovalFormTooLong { .. })
        ));
        assert_eq!(state_bytes(&fixture), before);

        let retry_args = json!({"request_ids":retry});
        assert!(confirmation(&mcp, &retry_args).unwrap().len() <= MAX_FORM);
        assert!(matches!(
            approve(&mcp, &retry_args, false),
            Err(Error::ApprovalRequired)
        ));
        assert_eq!(state_bytes(&fixture), before);
        approve(&mcp, &retry_args, true).unwrap();
        // Approving the prefix never answers any remaining request.
        for id in remaining {
            assert!(confirmation(&mcp, &json!({"request_id":id})).is_ok());
        }
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
        Outcome::Pending(id, _) => id,
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
        Outcome::Pending(..)
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
