//! Filesystem-backed request evaluation from the policy files and its boundaries.
use super::*;
use crate::{
    policy,
    review::{Checkpoint, tests::enrolled},
};
use std::fs;

fn evaluate(
    context: &Context,
    request: &policy::Request,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    let snapshot = consumer::load(context, checkpoint)?;
    let evaluated = snapshot.evaluate_controlled(request.clone(), checkpoint)?;
    Ok(json!({"identity":snapshot.metadata(),"evaluation":evaluated.evaluation()}))
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
fn requests_use_the_policy_files_and_refuse_other_projects() {
    let (_root, context) = enrolled();
    let evaluated = evaluate(&context, &request(&context), &|| Ok(())).unwrap();
    assert_eq!(
        evaluated["identity"]["policy_revision"],
        current_identity(&context)["policy_revision"]
    );
    let calls = std::cell::Cell::new(0);
    assert!(
        evaluate(&context, &request(&context), &|| {
            calls.set(calls.get() + 1);
            if calls.get() >= 2 {
                Err(policy::Error::Interrupted)
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    let mut other = context.clone();
    other.project_id = "a".repeat(64);
    assert!(evaluate(&other, &request(&other), &|| Ok(())).is_err());
    let mut wrong = request(&context);
    wrong.project = "/acme/other".into();
    assert!(evaluate(&context, &wrong, &|| Ok(())).is_err());
}

#[test]
fn checked_runs_decide_by_the_files_as_they_are_now() {
    let (root, context) = enrolled();
    let decision = |context: &Context| {
        evaluate(context, &request(context), &|| Ok(())).unwrap()["evaluation"]["decision"].clone()
    };
    assert_ne!(decision(&context), "allow");
    // A person adds the rule by hand; the next checked run follows it.
    fs::write(
        root.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".shield/rules.yml"),
        r#"{"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"acme-read","decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}]}"#,
    )
    .unwrap();
    assert_eq!(decision(&context), "allow");
}

#[test]
fn corrupt_review_state_blocks_without_mutation() {
    let (root, context) = enrolled();
    let file = root.path().join(".shield/state/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    state["project_id"] = json!("altered");
    fs::write(&file, state.to_string()).unwrap();
    assert!(evaluate(&context, &request(&context), &|| Ok(())).is_err());
    assert!(status(&context).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(),
        state
    );
}
