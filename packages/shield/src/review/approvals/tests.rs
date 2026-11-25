//! Real consumer resources, mixed atomic issuance and exact replay boundaries.
use super::*;
use crate::{
    policy::Request,
    review::{self, hook_approval, read_approval, run_approval},
    runtime::{EvaluatedRequest, Project},
};
use std::path::Path;

fn confirmation(context: &Context, args: &Value) -> Result<String, Error> {
    super::confirmation(context, args, &|| Ok(()))
}

fn approve(context: &Context, args: &Value, confirmed: bool) -> Result<Value, Error> {
    super::approve(context, args, confirmed, &|| Ok(()))
}

pub(crate) struct Fixture {
    pub(crate) root: assert_fs::TempDir,
    pub(crate) context: Context,
    pub(crate) read: EvaluatedRequest,
    pub(crate) read_digest: String,
    pub(crate) run: EvaluatedRequest,
    pub(crate) run_resource: Value,
    pub(crate) ids: Vec<String>,
}

pub(crate) fn fixture(agent: &str) -> Fixture {
    fixture_with_session(agent, Some("acme-session"))
}

fn fixture_with_session(agent: &str, session: Option<&str>) -> Fixture {
    let (root, mut context, read, read_digest) = read_approval::tests::fixture();
    context.agent = Some(agent.into());
    context.session_id = session.map(Into::into);
    let value = read_approval::gate(&context, "acme-factory", &read, &read_digest, None, || true)
        .expect("capture SDK read");
    let first = value["approval"]["request_id"]
        .as_str()
        .expect("read request ID")
        .to_owned();
    context.surface = "run".into();
    #[cfg(not(windows))]
    let (executable, arguments) = ("/bin/echo".into(), vec!["Acme text".into()]);
    #[cfg(windows)]
    let (executable, arguments) = (
        std::path::PathBuf::from(winsafe::GetSystemDirectory().expect("system directory"))
            .join("timeout.exe")
            .to_str()
            .expect("Unicode system executable")
            .into(),
        vec!["/t".into(), "0".into(), "/nobreak".into()],
    );
    let request = Request {
        executable,
        arguments,
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    let run_resource = crate::execution::approval_fixture(&request, 5000).expect("bind executable");
    let snapshot = Project::open(Path::new(&context.project_root))
        .expect("open project")
        .snapshot()
        .expect("load snapshot");
    let run = crate::execution::evaluate_approval(&snapshot, &request, &run_resource)
        .expect("evaluate command");
    let (value, outcome) = run_approval::gate(&context, &run, &run_resource, None, || true, |_| ())
        .expect("capture checked execution");
    assert!(outcome.is_none());
    let second = value["approval"]["request_id"]
        .as_str()
        .expect("execution request ID")
        .to_owned();
    context.surface = "mcp".into();
    Fixture {
        root,
        context,
        read,
        read_digest,
        run,
        run_resource,
        ids: vec![first, second],
    }
}

#[test]
fn action_request_kinds_large_mixed_form_splits_without_issuing_unseen_grants() {
    let f = fixture("codex");
    let read_form = confirmation(&f.context, &json!({"request_id":f.ids[0]})).unwrap();
    let run_form = confirmation(&f.context, &json!({"request_id":f.ids[1]})).unwrap();
    let mut request = f.run.request().clone();
    let padding = hook_approval::MAX_FORM - run_form.len() - read_form.len() / 2;
    request.arguments.extend(
        (0..padding.div_ceil(1024)).map(|index| "x".repeat((padding - index * 1024).min(1024))),
    );
    let resource = crate::execution::approval_fixture(&request, 5000).unwrap();
    let snapshot = Project::open(Path::new(&f.context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let run = crate::execution::evaluate_approval(&snapshot, &request, &resource).unwrap();
    let mut context = f.context.clone();
    context.surface = "run".into();
    let (captured, _) =
        run_approval::gate(&context, &run, &resource, None, || true, |_| ()).unwrap();
    let id = captured["approval"]["request_id"].as_str().unwrap();
    assert!(confirmation(&f.context, &json!({"request_id":id})).is_ok());
    let args = json!({"request_ids":[f.ids[0],id]});
    assert!(matches!(
        confirmation(&f.context, &args),
        Err(Error::ApprovalFormTooLong { .. })
    ));
    assert!(matches!(
        approve(&f.context, &args, true),
        Err(Error::ApprovalFormTooLong { .. })
    ));
    let state = review::project::checkpoint(&f.context).unwrap();
    assert!(!state.authorization_events.iter().any(|event| matches!(
        event["kind"].as_str(),
        Some("read_grant_issued" | "run_grant_issued")
    )));
}

#[test]
fn terminal_group_replay_returns_closed_outcomes_without_another_confirmation() {
    let f = fixture("codex");
    let args = json!({"request_ids":f.ids});
    approve(&f.context, &args, true).unwrap();
    assert_eq!(
        read_approval::gate(
            &f.context,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["allowed"],
        true
    );
    assert!(matches!(
        approve(&f.context, &args, false),
        Err(Error::Grouped { .. })
    ));
    let mut run = f.context.clone();
    run.surface = "run".into();
    assert_eq!(
        run_approval::gate(
            &run,
            &f.run,
            &f.run_resource,
            Some(&f.ids[1]),
            || true,
            |_| ()
        )
        .unwrap()
        .0["allowed"],
        true
    );
    assert!(confirmation(&f.context, &args).is_err());
    let receipt = approve(&f.context, &args, false).unwrap();
    assert_eq!(receipt["status"], "terminal");
    assert_eq!(receipt["allowed"], false);
    assert!(
        receipt["grants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["allowed"] == false && row.get("approval_id").is_none())
    );
}

#[test]
fn action_request_kinds_sdk_reviewer_can_deny_only_its_matching_session() {
    let mut f = fixture("claude-sdk");
    f.context.agent = Some("claude".into());
    let mut foreign = f.context.clone();
    foreign.session_id = Some("other-session".into());
    assert!(super::deny(&foreign, &f.ids[0], true).is_err());
    assert!(matches!(
        super::deny(&f.context, &f.ids[0], false),
        Err(Error::ApprovalRequired)
    ));
    assert_eq!(
        super::deny(&f.context, &f.ids[0], true).unwrap()["status"],
        "revoked"
    );
    assert!(confirmation(&f.context, &json!({"request_id":f.ids[0]})).is_err());
}

#[test]
fn action_request_kinds_approve_and_consume_read_and_run_once_without_policy_changes() {
    let f = fixture("codex");
    let before = review::project::checkpoint(&f.context)
        .unwrap()
        .policy_revision;
    let args = json!({"request_ids":f.ids});
    let text = confirmation(&f.context, &args).unwrap();
    assert!(
        text.contains("SDK text read")
            && text.contains("checked command")
            && text.contains("acme-factory")
    );
    assert!(matches!(
        approve(&f.context, &args, false),
        Err(Error::ApprovalRequired)
    ));
    let grants = approve(&f.context, &args, true).unwrap();
    assert_eq!(grants["grants"].as_array().unwrap().len(), 2);
    assert!(
        grants["grants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|grant| grant["next_action"].is_string())
    );
    assert!(approve(&f.context, &args, true).is_err());
    assert_eq!(
        review::project::checkpoint(&f.context)
            .unwrap()
            .policy_revision,
        before
    );
    let mut run_context = f.context.clone();
    run_context.surface = "run".into();
    let (result, outcome) = run_approval::gate(
        &run_context,
        &f.run,
        &f.run_resource,
        Some(&f.ids[1]),
        || true,
        |_| "owned handoff",
    )
    .unwrap();
    assert_eq!(result["code"], "allowed-once");
    assert_eq!(outcome, Some("owned handoff"));
    let read = read_approval::gate(
        &f.context,
        "acme-factory",
        &f.read,
        &f.read_digest,
        Some(&f.ids[0]),
        || true,
    )
    .unwrap();
    assert_eq!(read["code"], "allowed-once");
    assert_ne!(
        read_approval::gate(
            &f.context,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "allowed-once"
    );
    let (_, outcome) = run_approval::gate(
        &run_context,
        &f.run,
        &f.run_resource,
        Some(&f.ids[1]),
        || true,
        |_| (),
    )
    .unwrap();
    assert!(outcome.is_none());
}

#[test]
fn action_request_kinds_changed_file_or_foreign_session_issues_no_partial_group() {
    for mutation in ["file", "agent", "session", "missing-session"] {
        let mut f = fixture("codex");
        let args = json!({"request_ids":f.ids});
        confirmation(&f.context, &args).unwrap();
        match mutation {
            "file" => std::fs::write(f.root.path().join("acme.txt"), "changed").unwrap(),
            "agent" => f.context.agent = Some("claude".into()),
            "session" => f.context.session_id = Some("other-session".into()),
            "missing-session" => f.context.session_id = None,
            _ => unreachable!(),
        }
        assert!(approve(&f.context, &args, true).is_err(), "{mutation}");
        let state = review::project::checkpoint(&f.context).unwrap();
        for id in &f.ids {
            assert_eq!(request_view(&f.context, id).unwrap().1["status"], "pending");
        }
        assert!(!state.authorization_events.iter().any(|event| matches!(
            event["kind"].as_str(),
            Some("read_grant_issued" | "run_grant_issued")
        )));
    }
}

#[test]
fn action_request_kinds_consumer_and_window_mismatch_leave_the_exact_grant_usable() {
    let mut f = fixture("claude-sdk");
    let consumer = f.context.clone();
    f.context.agent = Some("claude".into());
    approve(&f.context, &json!({"request_id":f.ids[0]}), true).unwrap();
    assert_eq!(
        read_approval::gate(
            &consumer,
            "another-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    let mut changed = f.read.request().clone();
    changed.arguments.extend(["--limit".into(), "1".into()]);
    let mut resource = f.read.resource().unwrap().clone();
    if let crate::policy::resolution::normalized::Input::File { original_input, .. } =
        &mut resource.input
    {
        original_input["limit"] = json!(1);
    }
    let snapshot = Project::open(Path::new(&f.context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let evidence = snapshot.evaluate_resource(changed, resource).unwrap();
    assert_eq!(
        read_approval::gate(
            &consumer,
            "acme-factory",
            &evidence,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    assert_eq!(
        read_approval::gate(
            &consumer,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "allowed-once"
    );
}

#[test]
fn action_request_kinds_mixed_native_override_read_and_run_are_one_atomic_review() {
    use review::hook_approval::{Identity, Key, Outcome, Summary};
    let mut f = fixture("codex");
    let identity = Identity {
        activation_id: f.read.identity().activation_id(),
        policy_revision: f.read.identity().policy_revision(),
        candidate_digest: f.read.identity().candidate_digest(),
    };
    let key = Key {
        agent: "codex",
        session: "acme-session",
        cwd: &f.context.project_root,
        tool: "Bash",
        input_digest: "acme-native-operation".into(),
    };
    let summary = Summary {
        action: "Run git merge --ff-only acme-main".into(),
        reason: "a rule denies it".into(),
        overrides_deny: true,
        deny_rules: vec!["acme-deny".into()],
        settings_change: false,
        policy_change: true,
    };
    let root = Path::new(&f.context.project_root);
    let id = match review::hook_approval::refused(root, &key, &identity, &summary).unwrap() {
        Outcome::Pending(id, _) => id,
        Outcome::Allowed => panic!("no grant"),
    };
    f.ids.push(id);
    let args = json!({"request_ids":f.ids});
    let text = confirmation(&f.context, &args).unwrap();
    assert!(text.contains("overrides the rule that denies it"));
    assert!(text.contains("turn off Shield's protection"), "{text}");
    assert!(
        text.contains("1 may replace Shield's policy files"),
        "{text}"
    );
    assert!(!text.contains("No policy changes"), "{text}");
    let header = text.split("Request 1").next().unwrap();
    assert!(
        header.contains("1 override a deny; 0 change protected agent settings"),
        "{text}"
    );
    assert!(!text.contains("Allow the agent to do this once?"));
    assert_eq!(
        approve(&f.context, &args, true).unwrap()["grants"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(matches!(
        review::hook_approval::refused(root, &key, &identity, &summary).unwrap(),
        Outcome::Allowed
    ));
    assert!(!matches!(
        review::hook_approval::refused(root, &key, &identity, &summary).unwrap(),
        Outcome::Allowed
    ));
    assert!(matches!(
        approve(&f.context, &args, false),
        Err(Error::Grouped { .. })
    ));
    assert_eq!(
        read_approval::gate(
            &f.context,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["allowed"],
        true
    );
    let mut run_context = f.context.clone();
    run_context.surface = "run".into();
    let (allowed, _) = run_approval::gate(
        &run_context,
        &f.run,
        &f.run_resource,
        Some(&f.ids[1]),
        || true,
        |_| (),
    )
    .unwrap();
    assert_eq!(allowed["allowed"], true);
    let replay = approve(&f.context, &args, false).unwrap();
    assert_eq!(replay["allowed"], false);
    assert_eq!(replay["grants"].as_array().unwrap().len(), 3);
    assert!(
        replay["grants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["allowed"] == false && v.get("approval_id").is_none())
    );
    // Closed metadata replay does not consume the native follow-up capability.
    assert!(review::hook_approval::followup(root, &key).unwrap());
}

#[test]
fn action_request_kinds_stay_answerable_however_long_the_person_takes() {
    let f = fixture("codex");
    let shown = request_view(&f.context, &f.ids[0]).unwrap().1;
    assert!(shown.get("pending_expires_at_ms").is_none(), "{shown}");
    let later = shown["created_at_ms"].as_u64().unwrap() + 7 * 24 * 60 * 60 * 1000;
    let snapshot = Project::open(Path::new(&f.context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let state = review::project::checkpoint(&f.context).unwrap();
    read_approval::review_in(&state, &f.context, &f.ids[0], &snapshot, later).unwrap();
    run_approval::review_in(&state, &f.context, &f.ids[1], &snapshot, later).unwrap();
}

#[test]
fn action_request_kinds_absent_session_is_preserved_and_cannot_be_replaced_on_retry() {
    let f = fixture_with_session("codex", None);
    let grouped = json!({"request_ids":f.ids});
    assert!(matches!(
        confirmation(&f.context, &grouped),
        Err(Error::Grouped { .. })
    ));
    approve(&f.context, &json!({"request_id":f.ids[0]}), true).unwrap();
    let mut changed = f.context.clone();
    changed.session_id = Some("invented-session".into());
    assert_eq!(
        read_approval::gate(
            &changed,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "approval-stale"
    );
    assert_eq!(
        read_approval::gate(
            &f.context,
            "acme-factory",
            &f.read,
            &f.read_digest,
            Some(&f.ids[0]),
            || true
        )
        .unwrap()["code"],
        "allowed-once"
    );
}
