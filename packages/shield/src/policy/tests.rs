//! Exercises policy authority, strict parsing, literal argv, and deterministic composition.

use super::*;
use proptest::prelude::*;
use serde_json::{Value, json};

#[test]
fn normalized_nul_validation_distinguishes_escaped_text_in_payloads_and_context() {
    let request = request(&["inspect"]);
    for text in [r"\u0000", "\0"] {
        let input = normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: json!({"nested":[{"text":text}]}),
        };
        let mut context = normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        };
        assert_eq!(
            input.validate(&context, &request.project).is_ok(),
            text != "\0"
        );
        let input = normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: json!({}),
        };
        context.environment = Some(BTreeMap::from([("ACME_TEXT".into(), text.into())]));
        assert_eq!(
            input.validate(&context, &request.project).is_ok(),
            text != "\0"
        );
        context.environment = Some(BTreeMap::from([(text.into(), "Acme".into())]));
        assert_eq!(
            input.validate(&context, &request.project).is_ok(),
            text != "\0"
        );
        let input = normalized::Input::Unknown {
            host_tool: "Acme".into(),
            original_input: Value::Object(serde_json::Map::from_iter([(text.into(), json!(true))])),
        };
        context.environment = None;
        assert_eq!(
            input.validate(&context, &request.project).is_ok(),
            text != "\0"
        );
    }
}

#[test]
fn rule_and_precondition_nul_validation_preserves_literal_escape_matching() {
    let literal = r"\u0000";
    let mut doc = document("acme/escaped", vec![rule("inspect", "any", &[], "allow")]);
    doc["rules"][0]["reason"] = json!(literal);
    doc["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "preconditions-v1"
    ]);
    doc["preconditions"] = json!([{
        "id":"literal-check",
        "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},
        "requires_any":[{"arguments":{"kind":"exact","values":[literal]}}],
        "on_unmet":{"decision":"deny","message":literal}
    }]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    for (argument, expected) in [(literal, Decision::Allow), ("other", Decision::Deny)] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&[argument])
            )
            .unwrap()
            .decision,
            expected
        );
    }
    let mut bad_rule = doc.clone();
    bad_rule["rules"][0]["reason"] = json!("\0");
    assert!(Pack::from_yaml(&bad_rule.to_string()).is_err());
    doc["preconditions"][0]["on_unmet"]["message"] = json!("\0");
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
}

#[test]
fn option_allowlists_preserve_values_boundaries_and_unknown_options() {
    let mut safe = rule("inspect", "any", &[], "allow");
    safe["conditions"]["arguments"] = json!({"kind":"options","prefix":["log"],"flags":["--oneline","-p","-z"],"values":["-n","--format"],"operands":true});
    let mut doc = document("acme/options", vec![safe]);
    doc["capabilities"] = json!(["structured-rules-v1"]);
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    for argv in [
        vec!["log", "--oneline", "-n5", "HEAD"],
        vec!["log", "HEAD", "-pz"],
        vec!["log", "--format=%s"],
        vec!["log", "--", "--output=literal-path"],
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&argv)
            )
            .unwrap()
            .decision,
            Decision::Allow
        );
    }
    for argv in [
        vec!["log", "--output=acme"],
        vec!["log", "-n"],
        vec!["log", "--oneline=yes"],
        vec!["-c", "alias.log=!acme", "log"],
        vec!["log", "HEAD", "--unknown"],
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&argv)
            )
            .unwrap()
            .decision,
            Decision::Ask
        );
    }
}

#[test]
fn optional_long_values_and_matcher_contracts_are_explicit() {
    let mut inspect = rule("inspect", "any", &[], "allow");
    inspect["conditions"]["arguments"] = json!({"kind":"options","prefix":["status"],"flags":[],"values":[],"operands":false,"optional_values":["--porcelain"]});
    let mut doc = document("acme/optional", vec![inspect]);
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    for (args, expected) in [
        (vec!["status", "--porcelain"], Decision::Allow),
        (vec!["status", "--porcelain=v2"], Decision::Allow),
        (vec!["status", "--porcelain", "v2"], Decision::Ask),
        (vec!["status", "--", "--porcelain"], Decision::Ask),
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Strict,
                &request(&args)
            )
            .unwrap()
            .decision,
            expected
        );
    }
    for (field, value) in [
        ("prefix", json!(null)),
        ("flags", json!(["--porcelain", "--porcelain"])),
        ("flags", json!(["--bad=value"])),
        ("values", json!(null)),
        ("require_any", json!(["--porcelain", "--porcelain"])),
        ("require_any", json!(["--unknown"])),
        ("operands", json!(null)),
    ] {
        let mut bad = doc.clone();
        bad["rules"][0]["conditions"]["arguments"][field] = value;
        assert!(Pack::from_yaml(&bad.to_string()).is_err(), "{field}");
    }
    doc["rules"][0]["conditions"]["arguments"] = json!({"kind":"any"});
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1", "glob-v1"]);
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
}

#[test]
fn risk_tokens_refuse_allow_and_keep_whole_policy_comparisons_incomplete() {
    let mut risky = rule("force", "any", &[], "deny");
    risky["conditions"]["arguments"] =
        json!({"kind":"risk_tokens","prefix":["push"],"any_of":["--force","-f",{"prefix":"+"}]});
    let mut doc = document("acme/options", vec![risky]);
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    for argv in [
        vec!["push", "origin", "main", "--force"],
        vec!["push", "-vf", "origin"],
        vec!["push", "origin", "+HEAD:main"],
        vec!["push", "--", "origin", "+HEAD:main"],
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&argv)
            )
            .unwrap()
            .decision,
            Decision::Deny
        );
    }
    assert_eq!(
        evaluate(
            std::slice::from_ref(&pack),
            None,
            Profile::Recommended,
            &request(&["push", "--", "--force"])
        )
        .unwrap()
        .decision,
        Decision::Deny
    );
    let partition =
        partition::build(&[(&pack, Profile::Recommended)], &[], "/acme", "/acme").unwrap();
    assert_eq!(
        partition.incomplete,
        Some(partition::Incomplete::UnsupportedSemanticDomain)
    );
    doc["rules"][0]["decision"]["loose"] = json!("allow");
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
}

#[test]
fn required_flags_are_parsed_not_found_inside_option_values() {
    let mut inspect = rule("dry-run", "any", &[], "allow");
    inspect["conditions"]["arguments"] = json!({"kind":"options","prefix":["clean"],"flags":["-n","-q","--dry-run"],"values":["-e"],"operands":true,"require_any":["-n","--dry-run"]});
    let mut doc = document("acme/dry-run", vec![inspect]);
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    for (argv, expected) in [
        (vec!["clean", "-qn"], Decision::Allow),
        (vec!["clean", "--dry-run", "--", "-f"], Decision::Allow),
        (vec!["clean", "-en"], Decision::Ask),
        (vec!["clean", "-e", "-n"], Decision::Ask),
        (vec!["clean", "--", "-n"], Decision::Ask),
        (vec!["clean", "-q"], Decision::Ask),
    ] {
        assert_eq!(
            evaluate(
                std::slice::from_ref(&pack),
                None,
                Profile::Recommended,
                &request(&argv)
            )
            .unwrap()
            .decision,
            expected
        );
    }
    doc["rules"][0]["conditions"]["arguments"]["require_any"] = json!(["--unknown"]);
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
    doc["rules"][0]["conditions"]["arguments"]["require_any"] = json!(null);
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
}

#[test]
fn initial_policy_schema_combines_effects_and_dependencies_with_explicit_capabilities() {
    let mut operation = rule("network", "any", &[], "deny");
    operation["conditions"]["executable"] = json!("git");
    operation["conditions"]["possible_effects"] = json!(["network"]);
    let mut document = document("acme/workflow", vec![operation]);
    document["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "possible-effects-v1",
        "dependencies-v1"
    ]);
    document["dependencies"] = json!([{"pack":"acme/git","requirement":"^0.1"}]);
    let dependency = pack("acme/git", vec![]);
    let pack = Pack::from_yaml(&document.to_string()).unwrap();
    assert_eq!(pack.dependencies().len(), 1);
    let mut request = request(&["push"]);
    request.executable = "git".into();
    assert!(evaluate_selected(&[(&pack, Profile::Recommended)], &[], &request).is_err());
    assert_eq!(
        evaluate_selected(
            &[
                (&pack, Profile::Recommended),
                (&dependency, Profile::Recommended)
            ],
            &[],
            &request
        )
        .unwrap()
        .decision,
        Decision::Deny
    );
    for capabilities in [
        json!(["structured-rules-v1"]),
        json!([
            "structured-rules-v1",
            "argument-rules-v1",
            "possible-effects-v1"
        ]),
        json!(["structured-rules-v1", "dependencies-v1"]),
        json!(["unknown-v1", "dependencies-v1", "possible-effects-v1"]),
        json!([
            "unknown-v1",
            "possible-effects-v1",
            "dependencies-v1",
            "dependencies-v1"
        ]),
    ] {
        let mut invalid = document.clone();
        invalid["capabilities"] = capabilities;
        assert!(Pack::from_yaml(&invalid.to_string()).is_err());
    }
    document.as_object_mut().unwrap().remove("dependencies");
    assert!(Pack::from_yaml(&document.to_string()).is_err());
    document["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "possible-effects-v1"
    ]);
    assert!(Pack::from_yaml(&document.to_string()).is_ok());
}

#[test]
fn unsupported_policy_document_versions_refuse_literal_effect_and_dependency_content() {
    for version in [0, 2, 3, 255] {
        let mut pack = document("acme/test", vec![]);
        pack["schema_version"] = json!(version);
        assert!(Pack::from_yaml(&pack.to_string()).is_err());
        pack["capabilities"] = json!([
            "structured-rules-v1",
            "argument-rules-v1",
            "possible-effects-v1",
            "dependencies-v1"
        ]);
        pack["dependencies"] = json!([]);
        assert!(Pack::from_yaml(&pack.to_string()).is_err());
        let custom = json!({"schema_version":version,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]});
        assert!(CustomRules::from_yaml(&custom.to_string()).is_err());
    }
}

#[test]
fn effect_capability_shares_resolution_but_cannot_establish_allowance() {
    let mut effect_rule = rule("network", "any", &[], "deny");
    effect_rule["conditions"]["executable"] = json!("git");
    effect_rule["conditions"]["possible_effects"] = json!(["network"]);
    let mut doc = document("acme/effects", vec![effect_rule.clone()]);
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    assert!(Pack::from_yaml(&doc.to_string()).is_err());
    doc["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "possible-effects-v1"
    ]);
    let pack = Pack::from_yaml(&doc.to_string()).unwrap();
    let mut req = request(&["push", "origin", "main"]);
    req.executable = "git".into();
    let denied = evaluate_selected(&[(&pack, Profile::Recommended)], &[], &req).unwrap();
    assert_eq!(denied.decision, Decision::Deny);
    assert_eq!(denied.matches[0].rule_id, "network");
    assert_eq!(json!(denied)["schema_version"], 2);
    req.arguments = vec!["status".into()];
    assert_eq!(
        evaluate_selected(&[(&pack, Profile::Loose)], &[], &req)
            .unwrap()
            .decision,
        Decision::Ask
    );
    doc["rules"][0]["decision"] = json!({"loose":"allow","recommended":"allow","strict":"allow"});
    let permissive = Pack::from_yaml(&doc.to_string()).unwrap();
    req.arguments = vec!["push".into()];
    assert_eq!(
        evaluate_selected(&[(&permissive, Profile::Loose)], &[], &req)
            .unwrap()
            .decision,
        Decision::Ask
    );
    let custom = json!({"schema_version":1,"rules":[{"id":"guard","decision":"deny","reason":"Acme network guard","conditions":effect_rule["conditions"].clone()}],"kind":"custom","capabilities":["argument-rules-v1","possible-effects-v1","structured-rules-v1"]}).to_string();
    let custom = CustomRules::from_yaml(&custom).unwrap();
    assert_eq!(
        evaluate_selected_context(&[], &[&custom], &req, true, &|| Ok(()), false)
            .unwrap()
            .decision,
        Decision::Deny
    );
    for invalid in [
        json!([]),
        json!(["network", "network"]),
        json!(["unknown"]),
        Value::Null,
    ] {
        doc["rules"][0]["conditions"]["possible_effects"] = invalid;
        assert!(Pack::from_yaml(&doc.to_string()).is_err());
    }
}

#[test]
fn cooperative_interruption_stops_matching_and_resolution_without_a_decision() {
    let pack = pack(
        "acme/interrupt",
        (0..1000)
            .map(|index| rule(&format!("r{index}"), "any", &[], "allow"))
            .collect(),
    );
    let selected = [(&pack, Profile::Recommended)];
    let request = request(&["read"]);
    let complete = evaluate_selected(&selected, &[], &request).unwrap();
    for stop in [1, 50, 1500, 2500] {
        let visited = std::cell::Cell::new(0);
        let checkpoint = || {
            visited.set(visited.get() + 1);
            if visited.get() == stop {
                Err(Error::Interrupted)
            } else {
                Ok(())
            }
        };
        assert!(matches!(
            evaluate_selected_controlled(&selected, &[], &request, true, &checkpoint),
            Err(Error::Interrupted)
        ));
        assert_eq!(visited.get(), stop);
    }
    let controlled =
        evaluate_selected_controlled(&selected, &[], &request, true, &|| Ok(())).unwrap();
    assert_eq!(json!(controlled), json!(complete));
    assert_eq!(
        evaluate_selected(&selected, &[], &request)
            .unwrap()
            .decision,
        Decision::Allow
    );
}

#[test]
fn selected_evaluation_rejects_duplicate_sources_and_excess_custom_documents() {
    let pack = pack(
        "acme/test",
        vec![rule("operation", "exact", &["write"], "allow")],
    );
    let custom = CustomRules::from_yaml(
        &json!({"schema_version":1,"rules":[custom_rule("same","project","deny","exact",&["write"])],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]})
            .to_string(),
    )
    .unwrap();
    assert!(
        evaluate_selected(
            &[(&pack, Profile::Loose), (&pack, Profile::Strict)],
            &[],
            &request(&["write"])
        )
        .is_err()
    );
    assert!(evaluate_selected(&[], &[&custom, &custom], &request(&["write"])).is_err());
    assert!(matches!(
        evaluate_selected(&[], &[&custom; 2], &request(&["write"])),
        Err(Error::Limit)
    ));
}

fn rule(id: &str, kind: &str, values: &[&str], decision: &str) -> Value {
    let arguments = if kind == "any" {
        json!({"kind":kind})
    } else {
        json!({"kind":kind,"values":values})
    };
    json!({"id":id,"reason":"Acme test policy","conditions":{"domain":"argv","executable":"acme","arguments":arguments},"decision":{"loose":decision,"recommended":decision,"strict":decision}})
}

fn document(id: &str, rules: Vec<Value>) -> Value {
    let mut capabilities = BTreeSet::from(["structured-rules-v1".to_owned()]);
    for row in &rules {
        serde_json::from_value::<rules::Rule>(row.clone())
            .unwrap()
            .capabilities(&mut capabilities);
    }
    json!({"schema_version":1,"id":id,"version":"0.1.0","description":"Acme test pack","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":capabilities,"rules":Value::Array(rules),"kind":"pack"})
}

fn pack(id: &str, rules: Vec<Value>) -> Pack {
    Pack::from_yaml(&document(id, rules).to_string()).unwrap()
}

fn request(arguments: &[&str]) -> Request {
    let context = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    Request {
        executable: "acme".into(),
        arguments: arguments.iter().map(|arg| (*arg).into()).collect(),
        cwd: context.clone(),
        project: context,
    }
}

fn custom_rule(id: &str, scope: &str, decision: &str, kind: &str, values: &[&str]) -> Value {
    let mut value = rule(id, kind, values, decision);
    let object = value.as_object_mut().unwrap();
    if scope != "project" {
        object.insert("scope".into(), json!(scope));
    }
    object.insert("decision".into(), json!(decision));
    value
}

fn custom(rules: Vec<Value>) -> CustomRules {
    let mut source = rules::document(&rules).unwrap();
    source["rules"] = Value::Array(rules);
    CustomRules::from_yaml(&source.to_string()).unwrap()
}

#[test]
fn unknown_asks_and_nonmatching_packs_abstain() {
    let packs = [
        pack("acme/one", vec![rule("read", "exact", &["read"], "allow")]),
        pack("acme/two", vec![rule("write", "exact", &["write"], "deny")]),
    ];
    assert_eq!(
        evaluate(&packs, None, Profile::Recommended, &request(&["read"]))
            .unwrap()
            .decision,
        Decision::Allow
    );
    assert_eq!(
        evaluate(&packs, None, Profile::Strict, &request(&["unknown"]))
            .unwrap()
            .decision,
        Decision::Ask
    );
}

#[test]
fn narrow_pack_exception_wins_but_other_pack_can_deny() {
    let first = pack(
        "acme/one",
        vec![
            rule("broad", "any", &[], "deny"),
            rule("read", "exact", &["read"], "allow"),
        ],
    );
    assert_eq!(
        evaluate(&[first], None, Profile::Recommended, &request(&["read"]))
            .unwrap()
            .decision,
        Decision::Allow
    );
    let packs = [
        pack("acme/one", vec![rule("read", "exact", &["read"], "allow")]),
        pack("acme/two", vec![rule("broad", "any", &[], "deny")]),
    ];
    assert_eq!(
        evaluate(&packs, None, Profile::Recommended, &request(&["read"]))
            .unwrap()
            .decision,
        Decision::Deny
    );
}

#[test]
fn custom_authority_overrides_community_in_both_directions() {
    for (community, local, expected) in [
        ("deny", "allow", Decision::Allow),
        ("allow", "deny", Decision::Deny),
    ] {
        let packs = [pack(
            "acme/one",
            vec![rule("read", "exact", &["read"], community)],
        )];
        let rules = custom(vec![custom_rule(
            "override",
            "project",
            local,
            "exact",
            &["read"],
        )]);
        let result = evaluate(
            &packs,
            Some(&rules),
            Profile::Recommended,
            &request(&["read"]),
        )
        .unwrap();
        assert_eq!(result.decision, expected);
        assert!(
            !result
                .matches
                .iter()
                .find(|m| m.source == "acme/one")
                .unwrap()
                .applied
        );
    }
}

#[test]
fn specificity_is_structural_and_context_changes_eligibility() {
    let mut scoped = custom_rule("scoped", "project", "allow", "exact", &["read"]);
    let input = request(&["read"]);
    scoped["conditions"] = json!({"domain":"exact_request", "input":{"kind":"argv","executable":input.executable,"arguments":input.arguments,"original_input":{}}, "context":{"cwd":input.cwd,"environment":null}, "executable":null});
    let rules = custom(vec![
        custom_rule("unscoped", "project", "deny", "exact", &["read"]),
        scoped,
    ]);
    let input = request(&["read"]);
    assert_eq!(
        evaluate(&[], Some(&rules), Profile::Recommended, &input)
            .unwrap()
            .decision,
        Decision::Allow
    );
    let changed = Request {
        cwd: std::env::temp_dir().to_str().unwrap().into(),
        ..input
    };
    assert_eq!(
        evaluate(&[], Some(&rules), Profile::Recommended, &changed)
            .unwrap()
            .decision,
        Decision::Deny
    );
    let rules = custom(vec![
        custom_rule("prefix", "project", "deny", "prefix", &["read"]),
        custom_rule("exact", "project", "allow", "exact", &["read", "more"]),
    ]);
    assert_eq!(
        evaluate(
            &[],
            Some(&rules),
            Profile::Recommended,
            &request(&["read", "more"])
        )
        .unwrap()
        .decision,
        Decision::Allow
    );
}

#[test]
fn equal_specificity_ties_choose_severity_not_rule_id() {
    let packs = [pack(
        "acme/one",
        vec![
            rule("a", "exact", &["read"], "allow"),
            rule("z", "exact", &["read"], "deny"),
        ],
    )];
    assert_eq!(
        evaluate(&packs, None, Profile::Recommended, &request(&["read"]))
            .unwrap()
            .decision,
        Decision::Deny
    );
}

#[test]
fn argv_boundaries_and_executable_selectors_are_literal() {
    let packs = [pack(
        "acme/one",
        vec![rule("read", "exact", &["read", "a b"], "allow")],
    )];
    assert_eq!(
        evaluate(
            &packs,
            None,
            Profile::Recommended,
            &request(&["read", "a b"])
        )
        .unwrap()
        .decision,
        Decision::Allow
    );
    for arguments in [
        vec!["read", "a", "b"],
        vec!["read", "'a b'"],
        vec!["read", "a b", "--dangerous"],
    ] {
        assert_eq!(
            evaluate(&packs, None, Profile::Recommended, &request(&arguments))
                .unwrap()
                .decision,
            Decision::Ask
        );
    }
    let changed = Request {
        executable: "/bin/acme".into(),
        ..request(&["read", "a b"])
    };
    assert_eq!(
        evaluate(&packs, None, Profile::Recommended, &changed)
            .unwrap()
            .decision,
        Decision::Ask
    );
}

#[test]
fn pack_order_does_not_change_decision_or_revision() {
    let mut packs = [
        pack("acme/one", vec![rule("read", "exact", &["read"], "allow")]),
        pack("acme/two", vec![rule("read", "exact", &["read"], "ask")]),
    ];
    let before = evaluate(&packs, None, Profile::Recommended, &request(&["read"])).unwrap();
    packs.reverse();
    let after = evaluate(&packs, None, Profile::Recommended, &request(&["read"])).unwrap();
    let selected: Vec<_> = packs
        .iter()
        .map(|pack| (pack, Profile::Recommended))
        .collect();
    let explicit = evaluate_selected(&selected, &[], &request(&["read"])).unwrap();
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(explicit).unwrap()
    );
    assert_eq!(before.revision, after.revision);
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_ne!(
        after.revision,
        evaluate(&packs, None, Profile::Strict, &request(&["read"]))
            .unwrap()
            .revision
    );
}

#[test]
fn duplicate_pack_identity_is_rejected() {
    let packs = [pack("acme/one", vec![]), pack("acme/one", vec![])];
    assert!(matches!(
        evaluate(&packs, None, Profile::Recommended, &request(&[])),
        Err(Error::Contract(_))
    ));
}

#[test]
fn incompatible_or_ambiguous_pack_documents_are_rejected() {
    let base = document("acme/test", vec![rule("read", "exact", &["read"], "allow")]);
    for (field, value) in [
        ("schema_version", json!(2)),
        ("version", json!("latest")),
        ("engines", json!({"shield":">=99.0.0"})),
        // Only Shield's version can be checked, so other programs refuse.
        ("engines", json!({"shield":">=0.1.0","node":">=24"})),
        ("engines", json!({})),
        ("engines", json!(">=0.1.0")),
        ("engine", json!(">=0.1.0")),
        ("id", json!("git")),
        ("capabilities", json!(["execute-code"])),
        ("unknown", json!(true)),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        assert!(Pack::from_yaml(&changed.to_string()).is_err(), "{field}");
    }
    let mut reversed = base.clone();
    reversed["rules"][0]["decision"]["loose"] = json!("deny");
    assert!(Pack::from_yaml(&reversed.to_string()).is_err());
    let mut duplicate = base.clone();
    duplicate["rules"]
        .as_array_mut()
        .unwrap()
        .push(base["rules"][0].clone());
    assert!(Pack::from_yaml(&duplicate.to_string()).is_err());
    for yaml in [
        "schema_version: 1\nschema_version: 1",
        "---\na: b\n---\nc: d",
        "a: &x [one]\nb: *x",
        "a: !include secret.yml",
        "a: {<<: {b: c}}",
    ] {
        assert!(parse::<Value>(yaml).is_err());
    }
}

#[test]
fn json_policy_preparation_preserves_strict_syntax_and_budgets() {
    for document in [
        r#"{"key":1,"k\u0065y":2}"#,
        r#"{"nested":{"key":1,"key":2}}"#,
        r#"{"values":[{"key":1,"key":2}]}"#,
        r#"{"key":1} {}"#,
    ] {
        assert!(parse::<Value>(document).is_err(), "{document}");
    }
    for depth in [64, 65] {
        let document = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(parse::<Value>(&document).is_ok(), depth == 64);
        assert_eq!(parse_yaml::<Value>(&document).is_ok(), depth == 64);
    }
    for count in [49_999, 50_000] {
        let document = format!("[{}]", vec!["0"; count].join(","));
        assert!(parse::<Value>(&document).is_ok());
        assert!(parse_yaml::<Value>(&document).is_ok());
    }
    // Empty containers reach the event bound before the node bound.
    for count in [49_997, 49_998] {
        let document = format!("[{}]", vec!["[]"; count].join(","));
        assert!(parse::<Value>(&document).is_ok());
        assert!(parse_yaml::<Value>(&document).is_ok());
    }
    let document =
        Value::Object((0..25_000).map(|i| (format!("k{i}"), json!(0))).collect()).to_string();
    assert!(parse::<Value>(&document).is_ok());
    assert!(parse_yaml::<Value>(&document).is_ok());
    let document = json!({"key":"x".repeat(MAX_DOCUMENT_BYTES)}).to_string();
    assert!(matches!(parse::<Value>(&document), Err(Error::Limit)));
    assert_eq!(
        parse::<Value>(r#"{"nested":[true,null,"acme",42]}"#).unwrap(),
        parse::<Value>("nested: [true, null, acme, 42]").unwrap()
    );
    assert_eq!(
        parse::<Value>("{key: value}").unwrap(),
        json!({"key":"value"})
    );
    assert_eq!(
        parse::<Value>("\"key\": value").unwrap(),
        json!({"key":"value"})
    );
}

#[test]
fn yaml_tags_are_refused_including_core_schema_tags() {
    for document in [
        "key: !!str 1",
        "key: !!int \"1\"",
        "!!map {key: value}",
        "key: !acme value",
        "key: !<tag:yaml.org,2002:str> 1",
        "key: ! value",
    ] {
        assert!(parse::<Value>(document).is_err(), "{document}");
    }
    assert_eq!(
        parse::<Value>("key: \"!!str 1\"").unwrap(),
        json!({"key":"!!str 1"})
    );
}

#[test]
fn unsupported_scopes_fields_and_malformed_requests_fail() {
    for scope in ["user", "organization"] {
        assert!(CustomRules::from_yaml(&json!({"schema_version":1,"rules":[custom_rule("invalid", scope, "deny", "any", &[])],"kind":"custom","capabilities":["structured-rules-v1"]}).to_string()).is_err());
    }
    for value in [false, true] {
        let mut rule = custom_rule("invalid", "project", "deny", "any", &[]);
        rule["mandatory"] = json!(value);
        assert!(
            CustomRules::from_yaml(&json!({"schema_version":1,"rules":[rule],"kind":"custom","capabilities":["structured-rules-v1"]}).to_string())
                .is_err()
        );
    }
    for bad in [
        Request {
            executable: String::new(),
            ..request(&[])
        },
        Request {
            cwd: "relative".into(),
            ..request(&[])
        },
        request(&["bad\0argument"]),
        request(&[&"a".repeat(4097)]),
    ] {
        assert!(evaluate(&[], None, Profile::Recommended, &bad).is_err());
    }
    assert!(matches!(
        Pack::from_yaml(&" ".repeat(MAX_DOCUMENT_BYTES + 1)),
        Err(Error::Limit)
    ));
}

proptest! {
    #[test]
    fn semantic_profile_bounds_preserve_resolver_order(
        changes in prop::collection::vec((0u8..3, 0u8..3, 0u8..3), 1..20),
        reverse in any::<bool>(),
    ) {
        use super::profile_comparison::{self, Bound};
        let decisions = ["allow", "ask", "deny"];
        let rules = changes.iter().enumerate().map(|(index, (from,to,shape))| {
            let (kind, values): (&str, &[&str]) = match shape {
                0 => ("any", &[]), 1 => ("prefix", &["push"]), _ => ("exact", &["status"]),
            };
            let mut rule = rule(&format!("r{index}"), kind, values, "ask");
            rule["conditions"]["executable"] = json!("git");
            if index == 0 { rule["conditions"]["possible_effects"] = json!(["network"]); }
            let lower = (*from).min(*to);
            let upper = (*from).max(*to);
            rule["decision"]["loose"] = json!(decisions[usize::from(lower)]);
            rule["decision"]["recommended"] = json!(decisions[usize::from(lower)]);
            rule["decision"]["strict"] = json!(decisions[usize::from(upper)]);
            rule
        }).collect();
        let mut doc = document("acme/profile-proof", rules);
        doc["schema_version"] = json!(1);
        doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1", "possible-effects-v1"]);
        let pack = Pack::from_yaml(&doc.to_string()).unwrap();
        let custom = CustomRules::from_yaml(&json!({"schema_version":1,"rules":[{"id":"guard","decision":"deny","reason":"Acme guard","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["push","acme"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string()).unwrap();
        let before = [(&pack, if reverse { Profile::Strict } else { Profile::Loose })];
        let after = [(&pack, if reverse { Profile::Loose } else { Profile::Strict })];
        let bound = profile_comparison::profiles(&before, &after, &[&custom], &[&custom]);
        for args in [&[][..], &["status"][..], &["push"][..], &["push", "acme"][..], &["--unknown", "push"][..]] {
            let mut req = request(args); req.executable = "git".into();
            let a = evaluate_selected(&before, &[&custom], &req).unwrap().decision;
            let b = evaluate_selected(&after, &[&custom], &req).unwrap().decision;
            match bound {
                Some(Bound::Equivalent) => prop_assert_eq!(a,b),
                Some(Bound::NonExpansive) => prop_assert!(b >= a),
                Some(Bound::NonRestrictive) => prop_assert!(b <= a),
                None => {},
            }
        }
    }

    #[test]
    fn indexed_evaluation_preserves_complete_scan_evidence(
        executable in prop_oneof![Just("acme"), Just("other"), Just("absent")],
        arguments in prop::collection::vec("[a-z ;$()]{0,12}", 0..6),
        operation in prop_oneof![Just("read"), Just("write"), Just("")],
    ) {
        let mut unrelated = rule("other", "any", &[], "deny");
        unrelated["conditions"]["executable"] = json!("other");
        let packs = [pack("acme/one", vec![rule("base","any",&[],"ask"),
            rule("read","exact",&["read"],"allow"), rule("write","prefix",&["write"],"deny"), unrelated]),
            pack("acme/two",vec![rule("conflict","exact",&["read"],"deny")])];
        let customs = custom(vec![custom_rule("override","project","allow","exact",&["read"]),
            custom_rule("restriction","project","deny","prefix",&["write"])]);
        let mut input = request(&[]);
        input.executable = executable.into(); input.arguments = arguments;
        if !operation.is_empty() { input.arguments.insert(0,operation.into()); }
        let selected = [(&packs[0], Profile::Strict), (&packs[1], Profile::Loose)];
        let selected_custom = [&customs];
        let indexed = evaluate_selected_with(&selected,&selected_custom,&input,true).unwrap();
        let reference = evaluate_selected_with(&selected,&selected_custom,&input,false).unwrap();
        prop_assert_eq!(serde_json::to_value(indexed).unwrap(), serde_json::to_value(reference).unwrap());
    }

    #[test]
    fn arbitrary_literal_arguments_cannot_expand_an_exact_allowance(arguments in prop::collection::vec("[a-z ;$()]{0,32}", 0..12)) {
        let packs = [pack("acme/test", vec![rule("read", "exact", &["read"], "allow")])];
        let input = Request { arguments: arguments.clone(), ..request(&[]) };
        let result = evaluate(&packs, None, Profile::Recommended, &input).unwrap();
        prop_assert_eq!(result.decision == Decision::Allow, arguments == ["read"]);
    }
}

#[test]
fn fallback_rules_yield_to_other_packs_but_never_unlock_their_own_restrictions() {
    let fallback = |mut rule: Value| {
        rule["fallback"] = json!(true);
        rule
    };
    let baseline = || {
        pack(
            "acme/baseline",
            vec![
                fallback(rule("review", "any", &[], "ask")),
                rule("deny-wipe", "prefix", &["wipe"], "deny"),
                fallback(rule(
                    "review-wipe-dry",
                    "exact",
                    &["wipe", "--dry-run"],
                    "ask",
                )),
            ],
        )
    };
    let tool = || {
        pack(
            "acme/tool",
            vec![
                rule("install", "prefix", &["install"], "allow"),
                rule("wipe-dry", "exact", &["wipe", "--dry-run"], "allow"),
            ],
        )
    };
    let decide = |packs: &[Pack], argv: &[&str]| {
        evaluate(packs, None, Profile::Recommended, &request(argv))
            .unwrap()
            .decision
    };
    // Alone, a fallback rule decides like any other.
    assert_eq!(decide(&[baseline()], &["install"]), Decision::Ask);
    assert_eq!(decide(&[baseline()], &["wipe", "--dry-run"]), Decision::Ask);
    // Another pack's regular rule decides instead of it.
    assert_eq!(decide(&[baseline(), tool()], &["install"]), Decision::Allow);
    // Where no other pack decides, the fallback still applies.
    assert_eq!(decide(&[baseline(), tool()], &["build"]), Decision::Ask);
    // Yielding returns to the pack's own regular rules, so a fallback exception
    // never hides that pack's restriction from another pack's allowance.
    assert_eq!(
        decide(&[baseline(), tool()], &["wipe", "--dry-run"]),
        Decision::Deny
    );
    assert_eq!(
        decide(&[baseline(), tool()], &["wipe", "all"]),
        Decision::Deny
    );
    let evaluation = evaluate(
        &[baseline(), tool()],
        None,
        Profile::Recommended,
        &request(&["install"]),
    )
    .unwrap();
    let review = evaluation
        .matches
        .iter()
        .find(|m| m.rule_id == "review")
        .unwrap();
    assert!(review.fallback && !review.applied);
}

#[test]
fn fallback_is_a_declared_pack_marker() {
    let mut marked = rule("review", "any", &[], "ask");
    marked["fallback"] = json!(true);
    let mut doc = document("acme/fallback", vec![marked.clone()]);
    assert!(Pack::from_yaml(&doc.to_string()).is_ok());
    doc["capabilities"] = json!(["structured-rules-v1", "argument-rules-v1"]);
    assert!(
        Pack::from_yaml(&doc.to_string()).is_err(),
        "undeclared capability"
    );
    marked["fallback"] = json!(false);
    assert!(Pack::from_yaml(&document("acme/fallback", vec![marked.clone()]).to_string()).is_err());
    marked["fallback"] = json!(true);
    marked["decision"] = json!("ask");
    let custom = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1","fallback-rules-v1"],"rules":[marked]});
    assert!(CustomRules::from_yaml(&custom.to_string()).is_err());
}
