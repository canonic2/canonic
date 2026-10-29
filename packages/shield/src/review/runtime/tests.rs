//! Filesystem-backed activation, reload, boundary and recovery acceptance.
use super::*;
use crate::review::{
    self,
    tests::{enrolled, input},
};
use assert_fs::prelude::*;
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
fn evaluated_activation_validation_reuses_only_identical_candidates() {
    let (root, context) = enrolled();
    activate_current(&context, None);
    let mut state = Store::open(root.path()).unwrap().read().unwrap();
    let active = state.active.take().unwrap();
    active
        .validate_evaluated(&state, &state.policy_revision)
        .unwrap();
    assert!(active.validate_evaluated(&state, "wrong-revision").is_err());

    // A separately retained active snapshot is validated on its own revision,
    // even when the current candidate and its validated revision differ.
    let rules = policy::rules::document(&[]).unwrap().to_string();
    state.candidates.insert(
        "project".into(),
        json!(
            configuration::Bundle::capture(
                r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
                &[],
                Some(&rules),
            )
            .unwrap()
        ),
    );
    let revision = configuration::Evaluator::new(snapshots(&state.candidates).unwrap())
        .unwrap()
        .revision()
        .to_owned();
    active.validate_evaluated(&state, &revision).unwrap();
    let mut altered = active;
    altered.binding = "altered".into();
    assert!(altered.validate_evaluated(&state, &revision).is_err());
}

#[test]
fn proposals_require_atomic_operator_commit_and_reconnecting_reads_its_activation() {
    let (_root, context) = enrolled();
    assert!(evaluate(&context, &request(&context), &|| Ok(())).is_err());
    let first = activate_current(&context, None);
    assert!(first["activation_id"].is_string());
    assert_eq!(
        evaluate(&context, &request(&context), &|| Ok(())).unwrap()["evaluation"]["decision"],
        "ask"
    );
    let proposal = review::tests::propose(&context, &input("activate"), &|| Ok(())).unwrap();
    let proposal_id = proposal["proposal_id"].as_str().unwrap();
    assert_eq!(status(&context).unwrap()["pending_change"], false);
    assert_eq!(
        evaluate(&context, &request(&context), &|| Ok(())).unwrap()["evaluation"]["decision"],
        "ask"
    );
    review::history_settings(&context, Some(false), None, true, &|| Ok(())).unwrap();
    review::operator::commit_proposal_fixture(&context, proposal_id).unwrap();
    let receipt = review::tests::receipt(&context, proposal_id)["data"].clone();
    assert_eq!(receipt["status"], "applied");
    assert_eq!(
        review::tests::receipt(&context, proposal_id)["data"].clone(),
        receipt
    );
    let mut reconnected = context.clone();
    reconnected.surface = "mcp".into();
    let connection = review::mcp::Connection::new("runtime-test".into());
    assert_eq!(
        review::mcp::call(
            &reconnected,
            &connection,
            "policy_evaluate",
            &json!({"input":{"kind":"argv","executable":"acme","arguments":["read"],"original_input":{}},"context":{"cwd":reconnected.project_root,"environment":{}}}),
            &|| Ok(())
        )["data"]["decision"],
        "allow"
    );
    assert_eq!(status(&reconnected).unwrap()["pending_change"], false);
    assert_eq!(
        status(&context).unwrap()["authorization"]["kind"],
        "runtime_activated"
    );
    assert_eq!(
        review::tests::history(&context, None).unwrap()["events"],
        json!([])
    );
    assert!(
        review::mcp::call(
            &reconnected,
            &connection,
            "shield_runtime_activate",
            &json!({}),
            &|| Ok(())
        )["error"]
            .is_object()
    );
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
fn unactivated_state_and_unapproved_retained_edits_do_not_reload() {
    let (root, context) = enrolled();
    let file = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    state["active"] = Value::Null;
    fs::write(&file, state.to_string()).unwrap();
    assert!(status(&context).unwrap()["active"].is_null());
    let active = activate_current(&context, None);
    let mut state: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let rules =
        crate::policy::rules::document(&[json!(input("edit"))["change"]["rule"].clone()]).unwrap();
    state["candidates"]["project"] = json!(
        configuration::Bundle::capture(
            r#"{"schema_version":1,"packs":[],"rules":"rules.json"}"#,
            &[],
            Some(&rules.to_string()),
        )
        .unwrap()
    );
    let candidates = serde_json::from_value(state["candidates"].clone()).unwrap();
    state["policy_revision"] = json!(
        configuration::Evaluator::new(snapshots(&candidates).unwrap())
            .unwrap()
            .revision()
    );
    fs::write(&file, state.to_string()).unwrap();
    assert_eq!(status(&context).unwrap()["pending_change"], true);
    assert_eq!(
        evaluate(&context, &request(&context), &|| Ok(())).unwrap()["active"],
        active
    );
    assert_eq!(
        evaluate(&context, &request(&context), &|| Ok(())).unwrap()["evaluation"]["decision"],
        "ask"
    );
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

#[test]
fn concurrent_initial_activation_has_one_commit_and_abandoned_lock_preserves_snapshot() {
    let (root, context) = enrolled();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let context = context.clone();
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    match try_activate_current(&context, None) {
                        Err(Error::Locked) if std::time::Instant::now() < deadline => {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                        result => return result.unwrap(),
                    }
                }
            })
        })
        .collect();
    let joined: Vec<_> = handles
        .into_iter()
        .map(std::thread::JoinHandle::join)
        .collect();
    let results: Vec<_> = joined.into_iter().map(Result::unwrap).collect();
    assert!(results.iter().all(|r| *r == results[0]));
    let store = Store::open(root.path()).unwrap();
    assert_eq!(store.read().unwrap().authorization_events.len(), 1);
    root.child(".shield/review/state.lock")
        .write_str("abandoned-writer")
        .unwrap();
    assert!(matches!(
        publish_fixture(&context, "stale", "stale", None, &|| Ok(())),
        Err(Error::Locked)
    ));
    assert_eq!(
        evaluate(&context, &request(&context), &|| Ok(())).unwrap()["active"],
        results[0]
    );
    root.child(".shield/review/.state-interrupted.tmp")
        .write_str("partial")
        .unwrap();
    assert_eq!(status(&context).unwrap()["active"], results[0]);
}
