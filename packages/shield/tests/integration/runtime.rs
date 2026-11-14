//! The public consumer API reads the CLI's policy files and never fabricates a launch grant.
use shield::{
    enforcement::{self, AgentAccess},
    policy,
    runtime::Project,
};

#[test]
fn public_consumer_reads_the_cli_policy_and_never_hands_off_ask_or_operator_access() {
    if super::isolated_home() {
        return;
    }
    let root = assert_fs::TempDir::new().unwrap();
    let initialized = super::operator::interactive(&root, &["init"], "yes\n");
    assert_eq!(initialized["code"], 0, "{initialized}");
    let status = super::operator::query(&root, &["status"]);
    let digest = status["policy_files"]["digest"].as_str().unwrap();
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
    assert_eq!(evidence.identity().candidate_digest(), digest);
    project
        .with_current(&evidence, |request, evaluation| {
            assert_eq!(request.arguments, ["read"]);
            assert_eq!(evaluation.decision, policy::Decision::Ask);
        })
        .unwrap();
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
}
