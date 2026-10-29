//! Consumers retain exact evidence and cannot hand stale or foreign activations onward.
use super::*;
use crate::review::{
    self, runtime,
    tests::{enrolled, input},
};
use std::{cell::Cell, sync::mpsc, thread, time::Duration};

fn activate(context: &Context, previous: Option<&str>) -> Value {
    let status = runtime::status(context).unwrap();
    runtime::publish_fixture(
        context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        previous,
        &|| Ok(()),
    )
    .unwrap()
}
fn request(context: &Context) -> policy::Request {
    policy::Request {
        executable: "acme".into(),
        arguments: vec!["read".into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    }
}

#[test]
fn expiring_allow_is_revalidated_before_retained_evidence_reaches_a_consumer() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = assert_fs::TempDir::new().unwrap();
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 2;
    let expires = crate::evidence::format_utc(deadline);
    let rules = json!({"schema_version":1,"rules":[{"id":"temporary","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}},"expires_at":expires}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let project = Project::open(root.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evaluated = snapshot.evaluate(request(&context)).unwrap();
    assert_eq!(evaluated.evaluation().decision, policy::Decision::Allow);
    project.with_current(&evaluated, |_, _| ()).unwrap();
    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        < deadline
    {
        thread::sleep(Duration::from_millis(20));
    }
    let invoked = Cell::new(false);
    assert!(matches!(
        project.with_current(&evaluated, |_, _| invoked.set(true)),
        Err(RuntimeError::StaleSnapshot)
    ));
    assert!(!invoked.get());
    assert_eq!(
        snapshot
            .evaluate(request(&context))
            .unwrap()
            .evaluation()
            .decision,
        policy::Decision::Ask
    );
}

#[test]
fn prepared_snapshots_own_requests_and_reactivation_invalidates_even_equal_policy() {
    let (root, context) = enrolled();
    let project = Project::open(root.path()).unwrap();
    assert!(project.snapshot().is_err());
    let first = activate(&context, None);
    let snapshot = project.snapshot().unwrap();
    assert_eq!(snapshot.identity().project_id(), context.project_id);
    assert_eq!(
        snapshot.identity().project_root(),
        root.path().canonicalize().unwrap()
    );
    assert_eq!(
        snapshot.identity().candidate_digest(),
        first["candidate_digest"].as_str().unwrap()
    );
    let mut literal = request(&context);
    literal.arguments = vec![
        "space value".into(),
        "$(touch acme)".into(),
        "--token".into(),
        "private-value".into(),
    ];
    let evaluated = snapshot.evaluate(literal.clone()).unwrap();
    literal.arguments.clear();
    project
        .with_current(&evaluated, |request, evaluation| {
            assert_eq!(
                request.arguments,
                ["space value", "$(touch acme)", "--token", "private-value"]
            );
            assert_eq!(evaluation.decision, policy::Decision::Ask);
        })
        .unwrap();
    let second = activate(&context, first["activation_id"].as_str());
    assert_eq!(first["policy_revision"], second["policy_revision"]);
    assert_ne!(first["activation_id"], second["activation_id"]);
    let invoked = Cell::new(false);
    assert!(matches!(
        project.with_current(&evaluated, |_, _| invoked.set(true)),
        Err(RuntimeError::StaleSnapshot)
    ));
    assert!(!invoked.get());
    let current = project
        .snapshot()
        .unwrap()
        .evaluate(request(&context))
        .unwrap();
    project.with_current(&current, |_, _| ()).unwrap();
}

#[test]
fn review_changes_do_not_mutate_prepared_evaluator_and_handoff_refuses_foreign_project() {
    let (root, context) = enrolled();
    activate(&context, None);
    let project = Project::open(root.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evaluated = snapshot.evaluate(request(&context)).unwrap();
    let proposal = review::tests::propose(&context, &input("consumer"), &|| Ok(())).unwrap();
    let id = proposal["proposal_id"].as_str().unwrap();
    review::operator::commit_proposal_fixture(&context, id).unwrap();
    assert_eq!(
        snapshot
            .evaluate(request(&context))
            .unwrap()
            .evaluation()
            .decision,
        policy::Decision::Ask
    );
    assert!(matches!(
        project.with_current(&evaluated, |_, _| ()),
        Err(RuntimeError::StaleSnapshot)
    ));
    assert_eq!(
        snapshot
            .evaluate(request(&context))
            .unwrap()
            .evaluation()
            .decision,
        policy::Decision::Ask
    );
    assert_eq!(
        project
            .snapshot()
            .unwrap()
            .evaluate(request(&context))
            .unwrap()
            .evaluation()
            .decision,
        policy::Decision::Allow
    );
    let (other_root, other) = enrolled();
    activate(&other, None);
    let other_project = Project::open(other_root.path()).unwrap();
    assert!(
        other_project
            .with_current(&evaluated, |_, _| panic!(
                "foreign evidence reached callback"
            ))
            .is_err()
    );
    assert!(snapshot.evaluate(request(&other)).is_err());
}

#[test]
fn handoff_lock_prevents_activation_until_callback_finishes_and_unwinds_on_panic() {
    let (root, context) = enrolled();
    let first = activate(&context, None);
    let project = Project::open(root.path()).unwrap();
    let evaluated = project
        .snapshot()
        .unwrap()
        .evaluate(request(&context))
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    let context_for_thread = context.clone();
    let first_for_thread = first.clone();
    let handle = project
        .with_current(&evaluated, |_, _| {
            let handle = thread::spawn(move || {
                let status = runtime::status(&context_for_thread).unwrap();
                let result = runtime::publish_fixture(
                    &context_for_thread,
                    status["review"]["policy_revision"].as_str().unwrap(),
                    status["review"]["candidate_digest"].as_str().unwrap(),
                    first_for_thread["activation_id"].as_str(),
                    &|| Ok(()),
                );
                sender.send(result).unwrap();
            });
            assert!(matches!(
                receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
                Err(Error::Locked)
            ));
            assert_eq!(runtime::status(&context).unwrap()["active"], first);
            handle
        })
        .unwrap();
    handle.join().unwrap();
    assert_ne!(
        activate(&context, first["activation_id"].as_str())["activation_id"],
        first["activation_id"]
    );
    let current = project
        .snapshot()
        .unwrap()
        .evaluate(request(&context))
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            project
                .with_current(&current, |_, _| panic!("owned callback panic"))
                .unwrap();
        }))
        .is_err()
    );
    assert!(!root.path().join(".shield/review/state.lock").exists());
    assert!(
        project
            .with_current(&current, |_, _| Err::<(), _>("consumer failure"))
            .unwrap()
            .is_err()
    );
    assert!(!root.path().join(".shield/review/state.lock").exists());
}

#[test]
fn retained_evidence_cannot_cross_corruption_or_reenrollment_at_the_same_root() {
    let (root, context) = enrolled();
    activate(&context, None);
    let project = Project::open(root.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evaluated = snapshot.evaluate(request(&context)).unwrap();
    let state_path = root.path().join(".shield/review/state.json");
    let bytes = std::fs::read(&state_path).unwrap();
    project.with_current(&evaluated, |_, _| ()).unwrap();
    assert_eq!(std::fs::read(&state_path).unwrap(), bytes);
    std::fs::write(&state_path, "corrupt").unwrap();
    assert!(project.snapshot().is_err());
    assert!(
        project
            .with_current(&evaluated, |_, _| panic!("corrupt state reached callback"))
            .is_err()
    );
    assert_eq!(
        snapshot
            .evaluate(request(&context))
            .unwrap()
            .evaluation()
            .decision,
        policy::Decision::Ask
    );
    std::fs::remove_file(&state_path).unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    review::initialize(
        root.path(),
        std::collections::BTreeMap::from([("project".into(), json!(bundle))]),
    )
    .unwrap();
    let replacement = review::open(root.path()).unwrap();
    assert_ne!(replacement.project_id, context.project_id);
    activate(&replacement, None);
    assert!(project.snapshot().is_err());
    assert!(
        project
            .with_current(&evaluated, |_, _| panic!(
                "reenrollment reached old callback"
            ))
            .is_err()
    );
    Project::open(root.path()).unwrap().snapshot().unwrap();
}
