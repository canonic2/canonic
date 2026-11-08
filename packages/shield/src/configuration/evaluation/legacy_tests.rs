use super::*;
use crate::configuration::Candidate;
use serde_json::{Value, json};

fn capture(profile: &str) -> Snapshot {
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tool","version":"0.1.0",
        "description":"Acme tool","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":"*"},"capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"inspect","reason":"Inspect","decision":"allow",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}},
            {"id":"destroy","reason":"Destroy","decision":"deny",
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["destroy"]}}}]}).to_string();
    let config = json!({"schema_version":1,"packs":[{"id":"acme/tool","version":"0.1.0",
        "profile":profile,"path":"tool.yml","registry_identity":"acme-main"}]})
    .to_string();
    Candidate::from_yaml(&config)
        .unwrap()
        .capture(&[pack], None)
        .unwrap()
}

/// The exact historical struct field order and empty shadow list.
fn original_effective(snapshot: &Snapshot) -> String {
    format!(
        "{{\"schema_version\":1,\"kind\":\"effective-candidate\",\"scopes\":[{{\"scope\":\"project\",\"lock_revision\":\"{}\",\"lock\":{}}}],\"packs\":[{{\"scope\":\"project\",\"selection\":{},\"shadowed\":[]}}],\"custom_rules\":[]}}",
        snapshot.lock.revision().unwrap(),
        serde_json::to_string(&snapshot.lock).unwrap(),
        serde_json::to_string(&snapshot.lock.document.packs[0]).unwrap(),
    )
}

#[test]
fn legacy_composition_keeps_original_pack_revision_and_compiled_roundtrip() {
    let snapshot = capture("recommended");
    let original = original_effective(&snapshot);
    let expected = policy::fingerprint(original.as_bytes());
    let normal = Evaluator::new(vec![(Scope::Project, snapshot.clone())]).unwrap();
    assert_ne!(normal.revision(), expected);
    let evaluator =
        Evaluator::new_for_revision(vec![(Scope::Project, snapshot)], &expected).unwrap();
    assert_eq!(evaluator.revision(), expected);
    assert_eq!(
        serde_json::to_string(evaluator.effective()).unwrap(),
        original
    );
    let loaded =
        Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
    assert_eq!(loaded.revision(), expected);
    assert_eq!(serde_json::to_string(loaded.effective()).unwrap(), original);
    let project = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    for (operation, expected) in [
        ("inspect", Decision::Allow),
        ("destroy", Decision::Deny),
        ("unknown", Decision::Ask),
    ] {
        let request = Request {
            executable: "acme".into(),
            arguments: vec![operation.into()],
            cwd: project.clone(),
            project: project.clone(),
        };
        assert_eq!(evaluator.evaluate(&request).unwrap().decision, expected);
        assert_eq!(loaded.evaluate(&request).unwrap().decision, expected);
    }
}

#[test]
fn legacy_composition_requires_exact_source_and_never_drops_other_scopes() {
    let snapshot = capture("recommended");
    let expected = policy::fingerprint(original_effective(&snapshot).as_bytes());
    assert!(
        Evaluator::new_for_revision(vec![(Scope::Project, snapshot.clone())], &"0".repeat(64))
            .is_err()
    );
    assert!(
        Evaluator::new_for_revision(vec![(Scope::Project, capture("strict"))], &expected).is_err()
    );
    assert!(
        Evaluator::new_for_revision(
            vec![
                (Scope::Project, snapshot.clone()),
                (Scope::Project, snapshot)
            ],
            &expected
        )
        .is_err()
    );
}

#[test]
fn legacy_composition_rejects_nonempty_shadow_lists_and_keeps_normal_encoding() {
    let snapshot = capture("recommended");
    let normal = Evaluator::new(vec![(Scope::Project, snapshot.clone())]).unwrap();
    let restored =
        Evaluator::new_for_revision(vec![(Scope::Project, snapshot)], normal.revision()).unwrap();
    assert_eq!(
        serde_json::to_string(normal.effective()).unwrap(),
        serde_json::to_string(restored.effective()).unwrap()
    );
    let mut historical: Value =
        serde_json::from_str(&original_effective(&capture("recommended"))).unwrap();
    historical["packs"][0]["shadowed"] = json!([{"scope":"user"}]);
    assert!(serde_json::from_value::<Effective>(historical).is_err());
}
