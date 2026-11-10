//! Branch provenance is identical in direct and persisted compiled decisions.
use crate::policy::{self, Binding, Pack, Profile, Request, compiled::Plan};
use serde_json::{Value, json};

fn source() -> Value {
    json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme provenance fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]})
}

fn compare(source: &Value, id: &str, change: Value, arguments: &[&str]) -> Value {
    let mut overrides = json!({});
    overrides[id] = change;
    let binding: Binding = serde_json::from_value(json!({"overrides":overrides})).unwrap();
    let project_change = if source["preconditions"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|gate| gate["id"] == id)
    {
        json!({"on_unmet":"deny"})
    } else {
        json!("deny")
    };
    let project: Binding =
        serde_json::from_value(json!({"overrides":{id:project_change}})).unwrap();
    let mut pack = Pack::from_yaml_bound(&source.to_string(), &binding, true).unwrap();
    pack.bind_override_layers(false, &project.overrides, Some(&binding.overrides));
    let request = Request {
        executable: "acme".into(),
        arguments: arguments
            .iter()
            .map(|argument| (*argument).into())
            .collect(),
        cwd: crate::test_support::absolute("/acme").into_owned(),
        project: crate::test_support::absolute("/acme").into_owned(),
    };
    let direct = serde_json::to_value(
        policy::evaluate_selected(&[(&pack, Profile::Recommended)], &[], &request).unwrap(),
    )
    .unwrap();
    let plan = Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    let loaded = Plan::load(plan.bytes().to_vec()).unwrap();
    for plan in [&plan, &loaded] {
        let compiled =
            serde_json::to_value(plan.evaluate(&request, &|| Ok(()), true, true).unwrap()).unwrap();
        assert_eq!(compiled["matches"], direct["matches"]);
    }
    direct["matches"][0].clone()
}

#[test]
fn unmet_rules_and_default_branches_identify_the_effective_override_layer() {
    let mut source = source();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "requirements-v1"
    ]);
    source["rules"] = json!([{"id":"safe","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requirements":{"arguments":{"kind":"exact","values":["safe"]}},"on_unmet":{"decision":"deny"}}]);
    let unmet = compare(&source, "safe", json!({"on_unmet":"ask"}), &["unsafe"]);
    assert_eq!(unmet["branch"], "unmet");
    assert_eq!(
        unmet["decision_provenance"],
        json!({"profile":"recommended","source":"local","default_decision":"deny","before_override":"deny","final_decision":"ask","override_layer":"local"})
    );
    let met = compare(&source, "safe", json!({"on_unmet":"ask"}), &["safe"]);
    assert_eq!(met["branch"], "met");
    assert_eq!(
        met["decision_provenance"],
        json!({"profile":"recommended","source":"pack","default_decision":"allow","before_override":"deny","final_decision":"allow","override_layer":"local"})
    );
}

#[test]
fn preconditions_report_authored_and_overridden_unmet_decisions() {
    let mut source = source();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "preconditions-v1"
    ]);
    source["preconditions"] = json!([{"id":"safe","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requires_any":[{"arguments":{"kind":"exact","values":["safe"]}}],"on_unmet":{"decision":"deny"}}]);
    let matched = compare(&source, "safe", json!({"on_unmet":"ask"}), &["unsafe"]);
    assert_eq!(matched["precondition"], true);
    assert_eq!(matched["decision_provenance"]["source"], "local");
    assert_eq!(matched["decision_provenance"]["default_decision"], "deny");
    assert_eq!(matched["decision_provenance"]["final_decision"], "ask");
}

#[test]
fn shared_invocation_members_keep_the_source_body_override_origin() {
    let mut source = source();
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "invocation-groups-v1",
        "command-sets-v1"
    ]);
    source["invocations"] = json!({"globals":{"id_prefix":"g","segments":[[[],["--plain"]]]}});
    source["command_sets"] = json!({"inspection":{"status":"status-member"}});
    source["rules"] = json!([{"id":"inspect","invocation":"globals","command_set":"inspection","decision":"ask","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]);
    let matched = compare(&source, "inspect", json!("allow"), &["--plain", "status"]);
    assert_eq!(matched["rule_id"], "g1-status-member");
    assert_eq!(
        matched["decision_provenance"],
        json!({"profile":"recommended","source":"local","default_decision":"ask","before_override":"deny","final_decision":"allow","override_layer":"local"})
    );
}
