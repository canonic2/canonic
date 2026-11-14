//! Filesystem-backed activation, reload, boundary and recovery acceptance.
use super::*;
use crate::review::tests::enrolled;
use std::fs;

fn activate_current(context: &Context, previous: Option<&str>) -> Value {
    try_activate_current(context, previous).unwrap()
}

fn try_activate_current(context: &Context, previous: Option<&str>) -> Result<Value, Error> {
    let status = status(context)?;
    publish_fixture(
        context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        previous,
        &|| Ok(()),
    )
}

fn evaluate(
    context: &Context,
    request: &policy::Request,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    let snapshot = consumer::load(context, checkpoint)?;
    let evaluated = snapshot.evaluate_controlled(request.clone(), checkpoint)?;
    Ok(json!({"active":snapshot.metadata(),"evaluation":evaluated.evaluation()}))
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
fn stale_binding_wrong_predecessor_cancellation_and_mcp_cannot_activate() {
    let (_root, context) = enrolled();
    let status = status(&context).unwrap();
    let revision = status["review"]["policy_revision"].as_str().unwrap();
    let digest = status["review"]["candidate_digest"].as_str().unwrap();
    assert!(publish_fixture(&context, "stale", digest, None, &|| Ok(())).is_err());
    assert!(publish_fixture(&context, revision, "altered", None, &|| Ok(())).is_err());
    assert!(publish_fixture(&context, revision, digest, Some("other"), &|| Ok(())).is_err());
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    assert!(matches!(
        publish_fixture(&mcp, revision, digest, None, &|| Ok(())),
        Err(Error::ApprovalRequired)
    ));
    let calls = std::cell::Cell::new(0);
    assert!(
        publish_fixture(&context, revision, digest, None, &|| {
            calls.set(calls.get() + 1);
            if calls.get() >= 3 {
                Err(policy::Error::Interrupted)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert!(super::status(&context).unwrap()["active"].is_null());
    let active = activate_current(&context, None);
    assert_eq!(activate_current(&context, None), active);
    assert!(publish_fixture(&context, revision, digest, Some("wrong"), &|| Ok(())).is_err());
    let mut other = context.clone();
    other.project_id = "a".repeat(64);
    assert!(evaluate(&other, &request(&other), &|| Ok(())).is_err());
    let mut wrong = request(&context);
    wrong.project = "/acme/other".into();
    assert!(evaluate(&context, &wrong, &|| Ok(())).is_err());
}

#[test]
fn corrupt_active_state_blocks_without_mutation() {
    let (root, context) = enrolled();
    activate_current(&context, None);
    let file = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    state["active"]["candidate_digest"] = json!("altered");
    fs::write(&file, state.to_string()).unwrap();
    assert!(evaluate(&context, &request(&context), &|| Ok(())).is_err());
    assert!(status(&context).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(),
        state
    );
}
