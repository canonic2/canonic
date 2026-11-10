//! Invalid dynamic policy reports safe source locations without its values.
use crate::policy::{Binding, Error, Pack};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn document() -> Value {
    json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"0.1.0",
        "description":"Acme tools","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},
        "capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],
        "variables":{"profile":{"type":"string","description":"Profile","max_length":1024}},
        "rules":[{"id":"inspect","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]})
}

fn refusal(source: &Value, binding: &Binding) -> Error {
    match Pack::from_yaml_bound(&source.to_string(), binding, false) {
        Err(error) => error.in_selection("pack:acme-selection"),
        Ok(_) => panic!("invalid source was accepted"),
    }
}

fn location(error: &Error, entry: &str, field: &str, variable: &str) {
    let Error::Resolution {
        selection,
        entry: actual_entry,
        field: actual_field,
        variables,
        ..
    } = error
    else {
        panic!("expected typed resolution refusal: {error}");
    };
    assert_eq!(selection, "pack:acme-selection");
    assert_eq!(actual_entry, entry);
    assert_eq!(actual_field, field);
    assert!(variables.iter().any(|name| name == variable), "{error}");
}

#[test]
fn selector_when_and_optional_check_refusals_name_the_source_location() {
    let mut selector = document();
    selector["rules"][0]["conditions"]["executable"] = json!({"var":"missing"});
    location(
        &refusal(&selector, &Binding::default()),
        "inspect",
        "conditions",
        "missing",
    );

    let mut gated = document();
    gated["rules"][0]["when"] = json!([{"var":"missing","set":true}]);
    location(
        &refusal(&gated, &Binding::default()),
        "inspect",
        "when",
        "missing",
    );

    for optional in [
        json!({"values":[{"name":"--profile","match":{"var":"missing","unset":"any"}}]}),
        json!({"flags":["--profile"],"require_all":[{"name":"--profile","when":[{"var":"missing","set":true}]}]}),
        json!({"values":[{"name":"--profile","match":{"var":"profile","unset":"deny"}}]}),
    ] {
        let variable = if optional["values"][0]["match"]["var"] == "profile" {
            "profile"
        } else {
            "missing"
        };
        let mut source = document();
        let mut grammar = json!({"kind":"options","flags":[],"values":[],"operands":false});
        grammar
            .as_object_mut()
            .unwrap()
            .extend(optional.as_object().unwrap().clone());
        source["rules"][0]["conditions"]["arguments"] = grammar;
        location(
            &refusal(&source, &Binding::default()),
            "inspect",
            "conditions",
            variable,
        );
    }
}

#[test]
fn template_refusals_report_the_field_and_never_the_bound_value() {
    for field in ["reason", "guidance.message", "guidance.alternatives"] {
        let mut source = document();
        source["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!("templates-v1"));
        match field {
            "reason" => source["rules"][0]["reason"] = json!("Use {{missing}}"),
            "guidance.message" => {
                source["rules"][0]["guidance"] = json!({"message":"Use {{missing}}"});
            }
            _ => source["rules"][0]["guidance"] = json!({"alternatives":["Use {{missing}}"]}),
        }
        location(
            &refusal(&source, &Binding::default()),
            "inspect",
            field,
            "missing",
        );
    }
    let mut source = document();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("templates-v1"));
    source["rules"][0]["guidance"] = json!({"message":"Use {{profile}}"});
    let secret = "private-value-".repeat(50);
    let binding = Binding {
        variables: [("profile".into(), json!(secret))].into(),
        ..Binding::default()
    };
    let error = refusal(&source, &binding);
    location(&error, "inspect", "guidance.message", "profile");
    assert!(!error.to_string().contains("private-value"));
    assert!(
        error
            .to_string()
            .contains("rendered text exceeds field bounds")
    );
}

#[test]
fn precondition_projection_refusals_identify_the_entry_and_variable() {
    let mut source = document();
    source["capabilities"]
        .as_array_mut()
        .unwrap()
        .extend([json!("preconditions-v1"), json!("variable-parts-v1")]);
    source["preconditions"] = json!([{"id":"profile-part","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},
        "requires_any":[{"arguments":{"kind":"exact","values":[{"var":"profile","split":{"separator":"/","index":1}}]}}],"on_unmet":{"decision":"deny"}}]);
    let binding = Binding {
        variables: [("profile".into(), json!("private-value"))].into(),
        ..Binding::default()
    };
    let error = refusal(&source, &binding);
    location(&error, "profile-part", "requires_any", "profile");
    assert!(!error.to_string().contains("private-value"));
}

#[test]
fn locating_errors_preserves_syntax_resource_and_interruption_categories() {
    let names = BTreeSet::from(["profile".into()]);
    assert!(matches!(
        Error::Syntax.in_resolution("inspect", "conditions", &names),
        Error::Syntax
    ));
    assert!(matches!(
        Error::Limit.in_resolution("inspect", "conditions", &names),
        Error::Limit
    ));
    assert!(matches!(
        Error::Interrupted.in_resolution("inspect", "conditions", &names),
        Error::Interrupted
    ));
    let error = Error::Contract("reference to an undeclared variable").in_resolution(
        "unvalidated private text\n",
        "conditions",
        &BTreeSet::from(["private text\n".into()]),
    );
    assert!(!error.to_string().contains("private text"));
}
