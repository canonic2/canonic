//! Real retained-state fixtures exercise gated side effects and failure separation.
use crate::{
    configuration::Bundle,
    enforcement::*,
    policy::{Decision, Request},
    review,
    runtime::{self, EvaluatedRequest, Project},
};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap, fs};

fn fixture() -> (assert_fs::TempDir, review::Context, Project) {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"rules":[{"id":"read","decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read","space value","$(touch acme)"]}}},{"id":"delete","decision":"deny","reason":"Acme delete","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["delete"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]});
    let rules = rules.to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::initialize(
        root.path(),
        BTreeMap::from([("project".into(), json!(bundle))]),
    )
    .unwrap();
    let context = review::open(root.path()).unwrap();
    activate(&context, None);
    let project = Project::open(root.path()).unwrap();
    (root, context, project)
}

fn activate(context: &review::Context, previous: Option<&str>) -> Value {
    let status = review::runtime::status(context).unwrap();
    review::runtime::publish_fixture(
        context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        previous,
        &|| Ok(()),
    )
    .unwrap()
}

fn evidence(project: &Project, action: &str) -> EvaluatedRequest {
    let snapshot = project.snapshot().unwrap();
    let root = snapshot.identity().project_root().to_str().unwrap();
    snapshot
        .evaluate(Request {
            executable: "acme".into(),
            arguments: if action == "read" {
                vec![action.into(), "space value".into(), "$(touch acme)".into()]
            } else {
                vec![action.into()]
            },
            cwd: root.into(),
            project: root.into(),
        })
        .unwrap()
}

#[test]
fn deny_ask_and_unknown_coverage_never_perform_the_host_side_effect() {
    let (root, _, project) = fixture();
    let target = root.path().join("host-effect");
    for (action, expected) in [("delete", Decision::Deny), ("unknown", Decision::Ask)] {
        let evidence = evidence(&project, action);
        assert_eq!(evidence.evaluation().decision, expected);
        let result = handoff(&project, AgentAccess::Ordinary, evidence, |_, _| {
            fs::write(&target, "host effect").unwrap();
        });
        assert!(matches!(
            (expected, result),
            (Decision::Deny, Err(Error::Denied)) | (Decision::Ask, Err(Error::ApprovalRequired))
        ));
        assert!(!target.exists());
    }
    assert!(matches!(
        handoff(
            &project,
            AgentAccess::Unclassified,
            evidence(&project, "read"),
            |_, _| fs::write(&target, "unexpected callback").unwrap()
        ),
        Err(Error::Unclassified)
    ));
    assert!(!target.exists());
}

#[test]
fn allow_handoff_preserves_exact_input_and_host_failure_and_releases_lock() {
    let (root, _, project) = fixture();
    let invoked = Cell::new(0);
    let result = handoff(
        &project,
        AgentAccess::Ordinary,
        evidence(&project, "read"),
        |request, evaluation| {
            invoked.set(invoked.get() + 1);
            assert_eq!(request.executable, "acme");
            assert_eq!(request.arguments, ["read", "space value", "$(touch acme)"]);
            assert_eq!(evaluation.decision, Decision::Allow);
            Err::<(), _>("host operation failed")
        },
    )
    .unwrap();
    assert_eq!(result, Err("host operation failed"));
    assert_eq!(invoked.get(), 1);
    assert!(!root.path().join(".shield/review/state.lock").exists());
}

#[test]
fn policy_allow_cannot_authorize_classified_operator_or_state_access() {
    let (root, context, project) = fixture();
    let path = root.path().join(".shield/review/state.json");
    let original = fs::read(&path).unwrap();
    for access in [AgentAccess::Operator, AgentAccess::AuthorizationStateWrite] {
        let evidence = evidence(&project, "read");
        assert_eq!(evidence.evaluation().decision, Decision::Allow);
        assert!(matches!(
            handoff(&project, access, evidence, |_, _| {
                fs::write(&path, "agent tampering").unwrap();
            }),
            Err(Error::ProtectedAccess)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    // Independent operator CLI/core activation remains available.
    let active = review::runtime::status(&context).unwrap()["active"].clone();
    activate(&context, active["activation_id"].as_str());
}

#[test]
fn stale_foreign_and_corrupt_state_never_reach_allow_callback() {
    let (root, context, project) = fixture();
    let active = review::runtime::status(&context).unwrap()["active"].clone();
    let old = evidence(&project, "read");
    activate(&context, active["activation_id"].as_str());
    assert!(matches!(
        handoff(&project, AgentAccess::Ordinary, old, |_, _| panic!(
            "stale allow"
        )),
        Err(Error::Runtime(runtime::Error::StaleSnapshot))
    ));
    let (_other_root, _, other) = fixture();
    assert!(matches!(
        handoff(
            &other,
            AgentAccess::Ordinary,
            evidence(&project, "read"),
            |_, _| panic!("foreign allow")
        ),
        Err(Error::Runtime(runtime::Error::Invalid(_)))
    ));
    let current = evidence(&project, "read");
    fs::write(root.path().join(".shield/review/state.json"), "corrupt").unwrap();
    assert!(
        handoff(&project, AgentAccess::Ordinary, current, |_, _| panic!(
            "corrupt allow"
        ))
        .is_err()
    );
}

#[test]
fn allow_handoff_excludes_activation_until_the_callback_returns() {
    let (_root, context, project) = fixture();
    let active = review::runtime::status(&context).unwrap()["active"].clone();
    handoff(
        &project,
        AgentAccess::Ordinary,
        evidence(&project, "read"),
        |_, _| {
            let status = review::runtime::status(&context).unwrap();
            assert!(matches!(
                review::runtime::publish_fixture(
                    &context,
                    status["review"]["policy_revision"].as_str().unwrap(),
                    status["review"]["candidate_digest"].as_str().unwrap(),
                    active["activation_id"].as_str(),
                    &|| Ok(()),
                ),
                Err(review::Error::Locked)
            ));
        },
    )
    .unwrap();
    activate(&context, active["activation_id"].as_str());
}

#[test]
fn callback_panic_releases_the_writer_lock() {
    let (root, _, project) = fixture();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handoff(
                &project,
                AgentAccess::Ordinary,
                evidence(&project, "read"),
                |_, _| panic!("host panic"),
            )
            .unwrap();
        }))
        .is_err()
    );
    assert!(!root.path().join(".shield/review/state.lock").exists());
    handoff(
        &project,
        AgentAccess::Ordinary,
        evidence(&project, "read"),
        |_, _| (),
    )
    .unwrap();
}
