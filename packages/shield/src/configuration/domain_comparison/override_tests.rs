//! Override review compares prepared, composed policies rather than entry ranks.
use super::*;
use crate::configuration::{Bundle, Evaluator};
use serde_json::{Value, json};

fn pack() -> Value {
    json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Composed override fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["status"]}}}]})
}

fn snapshots(
    pack: &Value,
    overrides: Value,
    variables: Value,
    local: Option<Value>,
    custom: Option<Value>,
) -> Vec<(Scope, Snapshot)> {
    let mut selection =
        json!({"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"tools.yml"});
    selection["overrides"] = overrides;
    if variables.is_object() {
        selection["variables"] = variables;
    }
    let mut config = json!({"schema_version":1,"packs":[selection]});
    if custom.is_some() {
        config["rules"] = json!("rules.yml");
    }
    let local = local.map(|selection| json!({"schema_version":1,"packs":[selection]}).to_string());
    let custom = custom.map(|document| document.to_string());
    let bundle = Bundle::capture_layers(
        &config.to_string(),
        local.as_deref(),
        &[pack.to_string()],
        custom.as_deref(),
        None,
        &[],
    )
    .unwrap();
    vec![(Scope::Project, bundle.snapshot().unwrap())]
}

fn decision(snapshots: &[(Scope, Snapshot)], arguments: &[&str]) -> policy::Decision {
    let request = Request {
        executable: "acme".into(),
        arguments: arguments.iter().map(|arg| (*arg).into()).collect(),
        cwd: crate::test_support::absolute("/acme").into_owned(),
        project: crate::test_support::absolute("/acme").into_owned(),
    };
    let evaluator = Evaluator::new(snapshots.to_vec()).unwrap();
    let expected = evaluator.evaluate(&request).unwrap().decision;
    let loaded =
        Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
    assert_eq!(loaded.evaluate(&request).unwrap().decision, expected);
    expected
}

#[test]
fn override_reactivation_and_local_clear_prove_composed_expansion() {
    let source = pack();
    let before = snapshots(&source, json!({"inspect":"off"}), Value::Null, None, None);
    let after = snapshots(
        &source,
        json!({"inspect":"off"}),
        Value::Null,
        Some(json!({"id":"acme/tools","clear_overrides":["inspect"]})),
        None,
    );
    assert_eq!(decision(&before, &["status"]), policy::Decision::Ask);
    assert_eq!(decision(&after, &["status"]), policy::Decision::Allow);
    let report = compare_policy(&before, &after).unwrap();
    assert!(report.complete);
    assert_eq!(report.relation, Relation::Expansive);
    let witness = report.expansion.unwrap();
    assert_eq!(witness.request.arguments, ["status"]);
}

#[test]
fn override_widening_respects_variable_activation() {
    let mut source = pack();
    source["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1", "variables-v1"]);
    source["variables"] = json!({"tool":{"type":"choice","description":"Tool","values":["acme"]}});
    source["rules"][0]["conditions"]["executable"] = json!({"var":"tool"});
    let inactive = snapshots(&source, json!({"inspect":"deny"}), Value::Null, None, None);
    let relaxed = snapshots(&source, json!({"inspect":"allow"}), Value::Null, None, None);
    let report = compare_policy(&inactive, &relaxed).unwrap();
    assert!(report.complete);
    assert_eq!(report.relation, Relation::Equivalent);
    assert_eq!(decision(&relaxed, &["status"]), policy::Decision::Ask);
    let active = snapshots(
        &source,
        json!({"inspect":"allow"}),
        json!({"tool":"acme"}),
        None,
        None,
    );
    let report = compare_policy(&relaxed, &active).unwrap();
    assert!(report.complete);
    assert_eq!(report.relation, Relation::Expansive);
    assert!(report.expansion.is_some());
    assert_eq!(decision(&active, &["status"]), policy::Decision::Allow);
}

#[test]
fn custom_specificity_and_other_pack_restrictions_mask_entry_relaxation() {
    let source = pack();
    let custom = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"default","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}},{"id":"specific","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["status"]}}}]});
    let before = snapshots(
        &source,
        json!({"inspect":"deny"}),
        Value::Null,
        None,
        Some(custom.clone()),
    );
    let after = snapshots(
        &source,
        json!({"inspect":"allow"}),
        Value::Null,
        None,
        Some(custom),
    );
    let report = compare_policy(&before, &after).unwrap();
    assert!(report.complete);
    assert_eq!(report.relation, Relation::Equivalent);
    assert_eq!(decision(&after, &["status"]), policy::Decision::Deny);

    let mut restriction = pack();
    restriction["id"] = json!("acme/restriction");
    restriction["rules"][0]["decision"] = json!("deny");
    let composed = |override_decision: &str| {
        let config = json!({"schema_version":1,"packs":[
            {"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"tools.yml","overrides":{"inspect":override_decision}},
            {"id":"acme/restriction","version":"1.0.0","profile":"recommended","path":"restriction.yml"}
        ]});
        vec![(
            Scope::Project,
            Bundle::capture(
                &config.to_string(),
                &[source.to_string(), restriction.to_string()],
                None,
            )
            .unwrap()
            .snapshot()
            .unwrap(),
        )]
    };
    let before = composed("ask");
    let after = composed("allow");
    let report = compare_policy(&before, &after).unwrap();
    assert!(report.complete);
    assert_eq!(report.relation, Relation::Equivalent);
    assert_eq!(decision(&after, &["status"]), policy::Decision::Deny);
}

#[test]
fn unmet_override_changes_are_decided_with_the_successful_branch() {
    let mut source = pack();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "requirements-v1"
    ]);
    source["rules"][0]["conditions"]["arguments"] = json!({"kind":"any"});
    source["rules"][0]["requirements"] = json!({"arguments":{"kind":"exact","values":["status"]}});
    source["rules"][0]["on_unmet"] = json!({"decision":"deny"});
    let before = snapshots(&source, json!({}), Value::Null, None, None);
    let after = snapshots(
        &source,
        json!({"inspect":{"on_unmet":"ask"}}),
        Value::Null,
        None,
        None,
    );
    assert_eq!(decision(&before, &["status"]), policy::Decision::Allow);
    assert_eq!(decision(&after, &["status"]), policy::Decision::Allow);
    assert_eq!(decision(&before, &["other"]), policy::Decision::Deny);
    assert_eq!(decision(&after, &["other"]), policy::Decision::Ask);
    let report = compare_policy(&before, &after).unwrap();
    assert!(report.expansion.is_some());
    if report.complete {
        assert_eq!(report.relation, Relation::Expansive);
    } else {
        assert_eq!(report.relation, Relation::Unknown);
    }
}

#[test]
fn changed_preconditions_keep_override_witnesses_incomplete() {
    let mut source = pack();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "preconditions-v1"
    ]);
    source["preconditions"] = json!([{"id":"gate","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requires_any":[{"arguments":{"kind":"exact","values":["safe-before"]}}],"on_unmet":{"decision":"deny"}}]);
    let before = snapshots(&source, json!({}), Value::Null, None, None);
    source["preconditions"][0]["requires_any"][0]["arguments"]["values"] = json!(["safe-after"]);
    let after = snapshots(
        &source,
        json!({"gate":{"on_unmet":"ask"}}),
        Value::Null,
        None,
        None,
    );
    let report = compare_policy(&before, &after).unwrap();
    assert!(!report.complete);
    assert_eq!(report.relation, Relation::Unknown);
    let witness = report
        .expansion
        .expect("full policies decide the command example");
    let arguments: Vec<_> = witness
        .request
        .arguments
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(decision(&before, &arguments), policy::Decision::Deny);
    assert_eq!(decision(&after, &arguments), policy::Decision::Ask);
}
