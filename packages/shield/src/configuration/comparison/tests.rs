//! Verifies directional evidence, full bindings, and limits of request coverage.

use super::*;
use crate::{configuration::Candidate, policy::Decision};
use serde_json::{Value, json};

fn snapshot(_scope: Scope, decision: Decision) -> Snapshot {
    let rules = json!({"schema_version":1,"rules":[{"id":"operation","decision":decision,"reason":"Acme operation","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["write"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]});
    Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap()
        .capture(&[], Some(&rules.to_string()))
        .unwrap()
}

fn request() -> Request {
    let context = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    Request {
        executable: "acme".into(),
        arguments: vec!["write".into()],
        cwd: context.clone(),
        project: context,
    }
}

fn value(before: &[(Scope, Snapshot)], after: &[(Scope, Snapshot)], request: &Request) -> Value {
    serde_json::to_value(compare(before, after, request).unwrap()).unwrap()
}

#[test]
fn every_decision_transition_has_only_request_specific_direction() {
    for before_decision in [Decision::Allow, Decision::Ask, Decision::Deny] {
        for after_decision in [Decision::Allow, Decision::Ask, Decision::Deny] {
            let before = [(Scope::Project, snapshot(Scope::Project, before_decision))];
            let after = [(Scope::Project, snapshot(Scope::Project, after_decision))];
            let result = value(&before, &after, &request());
            let expected = match after_decision.cmp(&before_decision) {
                std::cmp::Ordering::Less => "expansion",
                std::cmp::Ordering::Greater => "restriction",
                std::cmp::Ordering::Equal => "unchanged",
            };
            assert_eq!(result["change"], expected);
            assert_eq!(result["overall_relation"], "unknown");
            assert_eq!(result["coverage"], "single-literal-request");
            assert_eq!(result["trust"], "unverified");
            assert_eq!(result["before"]["decision"], json!(before_decision));
            assert_eq!(result["after"]["decision"], json!(after_decision));
            assert_eq!(result, value(&before, &after, &request()));
        }
    }
}

#[test]
fn unchanged_sample_does_not_hide_other_expansions_or_claim_global_equivalence() {
    let before = [(Scope::Project, snapshot(Scope::Project, Decision::Deny))];
    let after = [(Scope::Project, snapshot(Scope::Project, Decision::Allow))];
    let mut unknown = request();
    unknown.arguments = vec!["write extra".into()];
    let result = value(&before, &after, &unknown);
    assert_eq!(result["change"], "unchanged");
    assert_eq!(result["before"]["decision"], "ask");
    assert_eq!(result["overall_relation"], "unknown");
    let expansion = value(&before, &after, &request());
    assert_eq!(expansion["change"], "expansion");
    assert_ne!(result["revision"], expansion["revision"]);
    assert_eq!(result["request"]["arguments"], json!(["write extra"]));
    assert_ne!(
        expansion["revision"],
        value(&after, &before, &request())["revision"]
    );
}

#[test]
fn evidence_binds_context_even_when_decisions_do_not_change_and_rejects_invalid_inputs() {
    let inputs = [(Scope::Project, snapshot(Scope::Project, Decision::Allow))];
    let mut changed = request();
    let original = value(&inputs, &inputs, &changed);
    changed.cwd.push_str("/acme");
    assert_ne!(
        original["revision"],
        value(&inputs, &inputs, &changed)["revision"]
    );
    changed = request();
    changed.project.push_str("/acme");
    assert_ne!(
        original["revision"],
        value(&inputs, &inputs, &changed)["revision"]
    );
    changed = request();
    changed.executable = "other".into();
    assert_ne!(
        original["revision"],
        value(&inputs, &inputs, &changed)["revision"]
    );
    changed.cwd = "relative".into();
    assert!(compare(&inputs, &inputs, &changed).is_err());
    assert!(compare(&[], &inputs, &request()).is_err());
    assert!(compare(&inputs, &[], &request()).is_err());
    let duplicated = [
        (Scope::Project, snapshot(Scope::Project, Decision::Allow)),
        (Scope::Project, snapshot(Scope::Project, Decision::Allow)),
    ];
    assert!(compare(&inputs, &duplicated, &request()).is_err());
}
