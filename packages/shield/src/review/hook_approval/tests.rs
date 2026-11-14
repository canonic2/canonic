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
fn a_copied_hook_grant_and_its_followup_are_each_spent_once_across_copies() {
    for consumed in [false, true] {
        let original = fixture();
        let id = pending(original.refuse("acme-copy"));
        approve(
            &original.connection("codex"),
            &json!({"request_id":id}),
            true,
        )
        .unwrap();
        if consumed {
            assert!(matches!(original.refuse("acme-copy"), Outcome::Allowed));
        }
        let root = assert_fs::TempDir::new().unwrap();
        crate::test_support::copy_fixture_directory(original.root(), root.path());
        let opened = review::open(root.path()).unwrap();
        let mut context = original.context.clone();
        context.project_id = opened.project_id;
        context.project_root = opened.project_root;
        let active = review::runtime::status(&context).unwrap()["active"].clone();
        let child = Fixture {
            _root: root,
            context,
            active,
        };
        // A copy is the same project with the same state.
        assert_eq!(child.context.project_id, original.context.project_id);
        if consumed {
            // The consumed grant's one follow-up is spent once, in either folder.
            assert!(followup(child.root(), &key("acme-copy")).unwrap());
            assert!(!followup(original.root(), &key("acme-copy")).unwrap());
            assert!(matches!(child.refuse("acme-copy"), Outcome::Pending(..)));
        } else {
            // The unused grant allows one retry, in whichever folder retries first.
            assert!(matches!(child.refuse("acme-copy"), Outcome::Allowed));
            assert!(matches!(original.refuse("acme-copy"), Outcome::Pending(..)));
            assert!(!followup(original.root(), &key("acme-copy")).unwrap());
        }
        // Either folder can still ask for and use a fresh approval.
        let fresh = pending(original.refuse("acme-copy"));
        assert_ne!(fresh, id);
        approve(
            &original.connection("codex"),
            &json!({"request_id":fresh}),
            true,
        )
        .unwrap();
        assert!(matches!(original.refuse("acme-copy"), Outcome::Allowed));
    }
}

#[test]
fn native_request_inspection_and_operator_revocation_share_the_request_family() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-revocation"));
    let listed = review::approvals::inspect(&fixture.context, None).unwrap();
    assert!(listed["requests"].as_array().unwrap().iter().any(|row| {
        row["request_id"] == id && row["type"] == "hook" && row["status"] == "pending"
    }));
    let (kind, shown) = review::approvals::request_view(&fixture.context, &id).unwrap();
    assert!(matches!(kind, review::approvals::RequestKind::Hook));
    assert_eq!(shown["provenance"]["session_id"], "acme-session");
    assert_eq!(shown["request"]["cwd"], "/workspace/acme");
    assert!(shown.get("input_digest").is_none());
    let mcp = fixture.connection("codex");
    approve(&mcp, &json!({"request_id":id}), true).unwrap();
    let mut operator = fixture.context.clone();
    operator.surface = "cli".into();
    let revoked = review::approvals::authorize(&operator, &id, true).unwrap();
    assert_eq!(revoked["status"], "revoked");
    let replay = review::approvals::authorize(&operator, &id, true).unwrap();
    assert_eq!(replay["status"], "revoked");
    let (_, receipt) = review::approvals::request_view(&operator, &id).unwrap();
    assert_eq!(receipt["status"], "revoked");
    assert!(matches!(
        fixture.refuse("acme-revocation"),
        Outcome::Pending(..)
    ));
}

#[test]
fn native_denial_requires_confirmation_and_preserves_consumer_session_ownership() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-denial"));
    let mcp = fixture.connection("codex");
    assert!(matches!(
        deny(&mcp, &id, false),
        Err(Error::ApprovalRequired)
    ));
    assert!(matches!(
        deny(&fixture.connection("claude-code"), &id, true),
        Err(Error::Missing)
    ));
    let mut other_session = mcp.clone();
    other_session.session_id = Some("another-session".into());
    assert!(matches!(
        deny(&other_session, &id, true),
        Err(Error::Missing)
    ));
    approve(&mcp, &json!({"request_id":id}), true).unwrap();
    assert!(matches!(fixture.refuse("acme-denial"), Outcome::Allowed));
    assert!(matches!(deny(&mcp, &id, true), Err(Error::Invalid(_))));
}

#[test]
fn denying_one_native_request_leaves_other_group_grants_usable() {
    let fixture = fixture();
    let first = pending(fixture.refuse("acme-first"));
    let second = pending(fixture.refuse("acme-second"));
    let mcp = fixture.connection("codex");
    approve(&mcp, &json!({"request_ids":[first,second]}), true).unwrap();
    deny(&mcp, &first, true).unwrap();
    assert!(matches!(fixture.refuse("acme-first"), Outcome::Pending(..)));
    assert!(matches!(fixture.refuse("acme-second"), Outcome::Allowed));
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
fn a_policy_change_or_upgrade_does_not_revoke_a_one_time_approval() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-kept"));
    approve(
        &fixture.connection("codex"),
        &json!({"request_id":id}),
        true,
    )
    .unwrap();
    // The retry is decided under a different policy revision, as after an
    // edit to the policy files or an upgrade that renames the generation.
    let changed = Identity {
        activation_id: "acme-changed",
        policy_revision: "acme-changed",
        candidate_digest: "acme-changed",
    };
    assert!(matches!(
        refused(fixture.root(), &key("acme-kept"), &changed, &summary()).unwrap(),
        Outcome::Allowed
    ));
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
    assert!(!followup(fixture.root(), &key("acme-discard")).unwrap());
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
    assert!(!followup(fixture.root(), &key("acme-escalated")).unwrap());
    assert!(matches!(fixture.refuse("acme-escalated"), Outcome::Allowed));
    let mut owner = mcp.clone();
    owner.session_id = Some("acme-session".into());
    // Reporting the already consumed decision must leave the separate callback
    // capability intact and must not persist a premature terminal receipt.
    let replay = review::approvals::approve(&owner, &json!({"request_id":id}), false).unwrap();
    assert_eq!(replay["status"], "consumed");
    assert_eq!(replay["allowed"], false);
    assert!(replay.get("approval_id").is_none());
    let state = review::project::checkpoint(&owner).unwrap();
    assert!(
        state
            .terminal_receipts
            .inspect(&owner, &id, now().unwrap())
            .is_none()
    );
    assert!(
        state
            .hook_requests
            .iter()
            .any(|row| row.request_id == id && row.live_id(now().unwrap()).is_some())
    );
    assert!(!followup(fixture.root(), &key("acme-other")).unwrap());
    assert!(followup(fixture.root(), &key("acme-escalated")).unwrap());
    assert!(!followup(fixture.root(), &key("acme-escalated")).unwrap());
    let state = review::project::checkpoint(&owner).unwrap();
    assert!(state.hook_requests.iter().all(|row| row.request_id != id));
    assert_eq!(
        state
            .terminal_receipts
            .inspect(&owner, &id, now().unwrap())
            .unwrap()
            .1["status"],
        "consumed"
    );
}

#[test]
fn native_consumed_receipt_deadline_is_anchored_before_delayed_harvest() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-expiry"));
    approve(
        &fixture.connection("codex"),
        &json!({"request_id":id}),
        true,
    )
    .unwrap();
    assert!(matches!(fixture.refuse("acme-expiry"), Outcome::Allowed));
    let mut state = review::project::checkpoint(&fixture.context).unwrap();
    let at = state
        .hook_requests
        .iter()
        .find(|r| r.request_id == id)
        .unwrap()
        .grant
        .as_ref()
        .unwrap()
        .consumed_at_ms
        .unwrap();
    assert!(
        state.hook_requests[0]
            .live_id(at + FOLLOWUP_MS - 1)
            .is_some()
    );
    state.compact_authorization_at(at + FOLLOWUP_MS);
    assert!(state.hook_requests.iter().all(|r| r.request_id != id));
    assert_eq!(
        state
            .terminal_receipts
            .inspect(&fixture.context, &id, at + 599_999)
            .unwrap()
            .1["status"],
        "consumed"
    );
    assert!(
        state
            .terminal_receipts
            .inspect(&fixture.context, &id, at + 600_000)
            .is_none()
    );
    let mut foreign = fixture.context.clone();
    foreign.surface = "mcp".into();
    foreign.agent = Some("codex".into());
    foreign.session_id = Some("another-session".into());
    assert!(
        state
            .terminal_receipts
            .inspect(&foreign, &id, at + FOLLOWUP_MS)
            .is_none()
    );
    foreign.surface = "cli".into();
    foreign.project_id = "f".repeat(64);
    assert!(
        state
            .terminal_receipts
            .inspect(&foreign, &id, at + FOLLOWUP_MS)
            .is_none()
    );
}

#[test]
fn new_native_denials_are_compact_and_replay_without_policy_preparation() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-denial-receipt"));
    let mut owner = fixture.connection("codex");
    owner.session_id = Some("acme-session".into());
    deny(&owner, &id, true).unwrap();
    let state = review::project::checkpoint(&owner).unwrap();
    assert!(state.hook_requests.iter().all(|row| row.request_id != id));
    assert_eq!(
        state
            .terminal_receipts
            .inspect(&owner, &id, now().unwrap())
            .unwrap()
            .1["status"],
        "revoked"
    );
    std::fs::write(fixture.root().join(".shield/shield.yml"), "invalid: [").unwrap();
    let replay = review::approvals::deny(&owner, &id, false).unwrap();
    assert_eq!(replay["status"], "revoked");
    assert_eq!(replay["allowed"], false);
    assert!(replay.get("approval_id").is_none());
    assert!(review::approvals::approve(&owner, &json!({"request_id":id}), false).is_err());
}

#[test]
fn legacy_native_revoked_rows_keep_their_existing_retention_deadline() {
    let fixture = fixture();
    let id = pending(fixture.refuse("acme-legacy-revoked"));
    let mut state = review::project::checkpoint(&fixture.context).unwrap();
    let row = state
        .hook_requests
        .iter_mut()
        .find(|row| row.request_id == id)
        .unwrap();
    row.status = "revoked".into();
    let until = row.pending_expires_at_ms + PENDING_MS;
    state.compact_authorization_at(until - 1);
    assert!(state.hook_requests.iter().any(|row| row.request_id == id));
    assert!(
        state
            .terminal_receipts
            .inspect(&fixture.context, &id, until - 1)
            .is_none()
    );
    state.compact_authorization_at(until);
    assert!(state.hook_requests.is_empty());
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
fn an_unenrolled_connection_of_the_requesting_agent_is_told_why_it_cannot_approve() {
    let fixture = fixture();
    let first = pending(fixture.refuse("acme-discard"));
    let second = pending(fixture.refuse("acme-other"));
    // The connection was launched for codex, but the project records no
    // integration for it, as in an unenrolled combined worktree.
    let mut unenrolled = fixture.context.clone();
    unenrolled.surface = "mcp".into();
    unenrolled.agent = None;
    unenrolled.declared_agent = Some("codex".into());
    let single = json!({"request_id":first});
    let call = |args: &Value| {
        review::mcp::call_with_confirmation(
            &unenrolled,
            &review::mcp::Connection::new("acme-connection".into()),
            "request_approve",
            args,
            &|| Ok(()),
            true,
        )
    };

    let refused = call(&single);
    assert_eq!(refused["error"]["code"], "agent_not_connected", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("shield mcp setup"),
        "{refused}"
    );
    let group = json!({"request_ids":[first, second]});
    let grouped = call(&group);
    assert_eq!(grouped["error"]["code"], "invalid_input", "{grouped}");
    let message = grouped["error"]["message"].as_str().unwrap();
    assert!(message.contains(&first), "{message}");
    assert!(message.contains("not connected to Shield"), "{message}");
    assert!(message.contains("shield mcp setup"), "{message}");
    assert!(matches!(
        review::approvals::confirmation(&unenrolled, &group),
        Err(Error::Grouped { request, .. }) if request == first
    ));
    assert!(matches!(
        deny(&unenrolled, &first, true),
        Err(Error::AgentNotConnected)
    ));

    // The listing marks what this connection cannot approve, and why.
    let listed = review::approvals::inspect(&unenrolled, None).unwrap();
    for row in listed["requests"].as_array().unwrap() {
        assert_eq!(row["approvable"], false, "{row}");
        assert!(
            row["approval_route"]
                .as_str()
                .unwrap()
                .contains("shield mcp setup"),
            "{row}"
        );
    }
    // The enrolled connection is offered both and still reaches the form.
    let enrolled = fixture.connection("codex");
    let listed = review::approvals::inspect(&enrolled, None).unwrap();
    assert!(
        listed["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("approvable").is_none()),
        "{listed}"
    );
    assert!(confirmation(&enrolled, &single).is_ok());

    // A different agent, enrolled or not, and an unknown ID stay not found.
    let mut other = unenrolled.clone();
    other.declared_agent = Some("claude".into());
    for context in [&other, &fixture.connection("claude")] {
        assert!(matches!(
            confirmation(context, &single),
            Err(Error::Missing)
        ));
    }
    assert!(matches!(
        review::approvals::inspect(&fixture.connection("claude"), None).unwrap()["requests"][0]
            ["approval_route"]
            .as_str(),
        Some(route) if !route.contains("shield mcp setup")
    ));
    assert!(matches!(
        confirmation(&unenrolled, &json!({"request_id":"hook-request-acme"})),
        Err(Error::Missing)
    ));
    // Nothing was issued.
    assert_eq!(pending(fixture.refuse("acme-discard")), first);
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
