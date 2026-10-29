//! The public consumer API shares CLI activation and never fabricates a launch grant.
use shield::{
    enforcement::{self, AgentAccess},
    policy,
    runtime::{Error, Project},
};

#[test]
fn public_consumer_prepares_current_cli_activation_and_rejects_stale_handoff() {
    let root = assert_fs::TempDir::new().unwrap();
    let initialized = super::operator::interactive(&root, &["init"], "yes\n");
    assert_eq!(initialized["code"], 0, "{initialized}");
    let status = super::operator::query(&root, &["status"]);
    let revision = status["active"]["policy_revision"].as_str().unwrap();
    let project = Project::open(root.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evidence = snapshot
        .evaluate(policy::Request {
            executable: "acme".into(),
            arguments: vec!["read".into()],
            cwd: snapshot.identity().project_root().to_str().unwrap().into(),
            project: snapshot.identity().project_root().to_str().unwrap().into(),
        })
        .unwrap();
    assert_eq!(evidence.identity().policy_revision(), revision);
    project
        .with_current(&evidence, |request, evaluation| {
            assert_eq!(request.arguments, ["read"]);
            assert_eq!(evaluation.decision, policy::Decision::Ask);
        })
        .unwrap();
    let gate_evidence = snapshot.evaluate(evidence.request().clone()).unwrap();
    assert!(matches!(
        enforcement::handoff(
            &project,
            AgentAccess::Ordinary,
            snapshot.evaluate(evidence.request().clone()).unwrap(),
            |_, _| panic!("ask reached host operation")
        ),
        Err(enforcement::Error::ApprovalRequired)
    ));
    assert!(matches!(
        enforcement::handoff(
            &project,
            AgentAccess::Operator,
            snapshot.evaluate(evidence.request().clone()).unwrap(),
            |_, _| panic!("agent reached operator action")
        ),
        Err(enforcement::Error::ProtectedAccess)
    ));
    let changed = super::operator::interactive(
        &root,
        &["rule", "add"],
        "other\nargv\nacme\n[\"other\"]\ndeny\nAcme other action\n\nyes\n",
    );
    assert_eq!(changed["code"], 0, "{changed}");
    assert!(matches!(
        project.with_current(&evidence, |_, _| panic!("stale evidence reached callback")),
        Err(Error::StaleSnapshot)
    ));
    assert!(matches!(
        enforcement::handoff(
            &project,
            AgentAccess::Ordinary,
            gate_evidence,
            |_, _| panic!("stale gate reached host")
        ),
        Err(enforcement::Error::Runtime(Error::StaleSnapshot))
    ));
}
