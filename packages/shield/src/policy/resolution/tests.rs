//! Exercises variable resolution, activation, templates, preconditions and overrides
//! through pack preparation and the shared resolver.

use super::super::{Decision, Pack, Profile, Request, compiled::Plan, evaluate};
use super::{Binding, Override};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn document(capabilities: &[&str], variables: Value, rules: Value, preconditions: Value) -> String {
    let mut doc = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"0.1.0","description":"Acme tools","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":capabilities});
    doc["rules"] = rules;
    if !variables.is_null() {
        doc["variables"] = variables;
    }
    if !preconditions.is_null() {
        doc["preconditions"] = preconditions;
    }
    doc.to_string()
}

fn argv(executable: &str, arguments: Value) -> Value {
    let mut conditions = json!({"domain":"argv","executable":executable});
    conditions["arguments"] = arguments;
    conditions
}

fn rule(id: &str, executable: Value, arguments: Value, decision: &str) -> Value {
    let mut conditions = json!({"domain":"argv"});
    conditions["executable"] = executable;
    conditions["arguments"] = arguments;
    json!({"id":id,"decision":decision,"conditions":conditions})
}

fn bound(variables: Value) -> Binding {
    Binding {
        variables: serde_json::from_value(variables).unwrap(),
        overrides: BTreeMap::new(),
    }
}

fn request(command: &[&str]) -> Request {
    Request {
        executable: command[0].into(),
        arguments: command[1..].iter().map(|a| (*a).into()).collect(),
        cwd: "/acme/project".into(),
        project: "/acme/project".into(),
    }
}

fn decide(pack: &Pack, command: &[&str]) -> Decision {
    evaluate(
        std::slice::from_ref(pack),
        None,
        Profile::Recommended,
        &request(command),
    )
    .unwrap()
    .decision
}

const NODE_CAPABILITIES: &[&str] = &[
    "structured-rules-v1",
    "argument-rules-v1",
    "variables-v1",
    "preconditions-v1",
    "templates-v1",
];

fn node_pack() -> String {
    document(
        NODE_CAPABILITIES,
        json!({"package_manager":{"type":"choice","description":"Package manager agents use","values":["npm","pnpm","yarn"],"suggest":[{"file":"pnpm-lock.yaml","value":"pnpm"}]}}),
        json!([
            rule("install", json!({"one_of":["npm","pnpm","yarn"]}), json!({"kind":"exact","values":["install"]}), "allow"),
            rule("npx", json!("npx"), json!({"kind":"any"}), "allow"),
            {"id":"npx-with-pnpm","when":[{"var":"package_manager","equals":"pnpm"}],"decision":"deny","reason":"Use {{package_manager}} dlx.","conditions":argv("npx", json!({"kind":"any"}))}
        ]),
        json!([{"id":"package-manager","conditions":argv("npm", json!({"kind":"any"})),"requires_any":[{"executable":{"var":"package_manager"}}],"on_unmet":{"decision":"deny","message":"This project uses {{package_manager}}.","alternatives":["Run the same command with {{package_manager}}."]}}]),
    )
    .replace(
        r#""executable":"npm","arguments":{"kind":"any"}"#,
        r#""executable":{"one_of":["npm","pnpm","yarn"]},"arguments":{"kind":"any"}"#,
    )
}

#[test]
fn unset_variables_leave_their_entries_inactive_and_packs_unchanged() {
    let pack = Pack::from_yaml(&node_pack()).unwrap();
    for command in [["npm", "install"], ["pnpm", "install"], ["yarn", "install"]] {
        assert_eq!(decide(&pack, &command), Decision::Allow);
    }
    assert_eq!(decide(&pack, &["npx", "acme"]), Decision::Allow);
    let states: Vec<_> = pack
        .resolution()
        .iter()
        .map(|e| (e.id.as_str(), e.state, e.reason.clone()))
        .collect();
    assert!(states.contains(&(
        "package-manager",
        "inactive",
        Some("package_manager unset".into())
    )));
    assert!(states.contains(&(
        "npx-with-pnpm",
        "inactive",
        Some("when test on package_manager does not hold".into())
    )));
}

#[test]
fn bound_choice_gates_other_managers_and_activates_conditional_rules() {
    let source = node_pack();
    let pack =
        Pack::from_yaml_bound(&source, &bound(json!({"package_manager":"pnpm"})), true).unwrap();
    assert_eq!(decide(&pack, &["pnpm", "install"]), Decision::Allow);
    assert_eq!(decide(&pack, &["pnpm", "publish"]), Decision::Ask);
    assert_eq!(decide(&pack, &["npx", "acme"]), Decision::Deny);
    let denied = evaluate(
        std::slice::from_ref(&pack),
        None,
        Profile::Recommended,
        &request(&["npm", "install"]),
    )
    .unwrap();
    assert_eq!(denied.decision, Decision::Deny);
    let reported = serde_json::to_value(&denied).unwrap();
    assert_eq!(
        reported["guidance"][0]["message"],
        "This project uses pnpm."
    );
    assert_eq!(
        reported["guidance"][0]["alternatives"][0],
        "Run the same command with pnpm."
    );
    let decided: Vec<_> = reported["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["rule_id"].clone(), m["precondition"].clone()))
        .collect();
    assert_eq!(decided, vec![(json!("package-manager"), json!(true))]);
    let npm =
        Pack::from_yaml_bound(&source, &bound(json!({"package_manager":"npm"})), true).unwrap();
    assert_eq!(decide(&npm, &["npm", "install"]), Decision::Allow);
    assert_eq!(decide(&npm, &["npx", "acme"]), Decision::Allow);
    assert_ne!(pack.digest, npm.digest);
}

#[test]
fn failed_preconditions_outrank_specific_allowances_and_never_allow() {
    let source = document(
        &["structured-rules-v1", "argument-rules-v1", "variables-v1", "preconditions-v1"],
        json!({"profile":{"type":"string","description":"AWS profile agents use","layer":"local","match":{"regex":"[a-z-]+"}}}),
        json!([
            rule("list", json!("aws"), json!({"kind":"exact","values":["s3","ls"]}), "allow"),
            rule("list-profiled", json!("aws"), json!({"kind":"options","prefix":["s3","ls"],"flags":[],"values":["--profile"],"operands":false}), "allow"),
            rule("remove", json!("aws"), json!({"kind":"prefix","values":["s3","rm"]}), "ask")
        ]),
        json!([{"id":"aws-profile","conditions":argv("aws", json!({"kind":"any"})),"requires_any":[{"arguments":{"kind":"options","prefix":[],"flags":[],"values":[{"name":"--profile","match":{"var":"profile"}}],"operands":true,"require_all":["--profile"]}}],"on_unmet":{"decision":"deny","message":"Use the {{profile}} AWS profile."}}]),
    )
    .replace(r#""capabilities":["structured-rules-v1""#, r#""capabilities":["regex-v1","structured-rules-v1""#);
    let open = Pack::from_yaml(&source).unwrap();
    assert_eq!(decide(&open, &["aws", "s3", "ls"]), Decision::Allow);
    let pack =
        Pack::from_yaml_bound(&source, &bound(json!({"profile":"acme-read-only"})), true).unwrap();
    assert_eq!(decide(&pack, &["aws", "s3", "ls"]), Decision::Deny);
    assert_eq!(
        decide(&pack, &["aws", "s3", "ls", "--profile", "acme-read-only"]),
        Decision::Allow
    );
    assert_eq!(
        decide(
            &pack,
            &["aws", "s3", "rm", "x", "--profile", "acme-read-only"]
        ),
        Decision::Ask
    );
    assert_eq!(
        decide(&pack, &["aws", "s3", "ls", "--profile", "acme-admin"]),
        Decision::Deny
    );
    for invalid in [json!({"profile":"Admin"}), json!({"profile":"token=acme"})] {
        assert!(Pack::from_yaml_bound(&source, &bound(invalid), true).is_err());
    }
}

/// The option scan an argparse-style CLI needs: global options anywhere,
/// abbreviated, between service options the pack does not declare.
fn global_scan(profile: &Value, extra: &Value) -> Value {
    let mut scan = json!({"kind":"options","prefix":[],"flags":["--debug"],
        "values":[{"name":"--profile","match":profile},"--region"],
        "operands":true,"unknown_options":"skip","long_abbreviations":true,
        "require_all":["--profile"]});
    for (key, value) in extra.as_object().into_iter().flatten() {
        scan[key] = value.clone();
    }
    scan
}

fn profile_gate_source() -> String {
    let gate = |id: &str, var: &str, profile: Value, message: &str| {
        json!({"id":id,"when":[{"var":var,"set":true}],
            "conditions":argv("aws", json!({"kind":"any"})),
            "requires_any":[{"arguments":global_scan(&profile, &json!({}))}],
            "on_unmet":{"decision":"deny","message":message}})
    };
    document(
        &[
            "structured-rules-v1",
            "argument-rules-v1",
            "variables-v1",
            "preconditions-v1",
            "unknown-options-v1",
            "option-abbreviations-v1",
            "negated-selectors-v1",
            "templates-v1",
        ],
        json!({
            "profile":{"type":"string","description":"AWS profile agents must use"},
            "blocked_profiles":{"type":"list","description":"AWS profiles agents must not use","items":{"type":"string"}}
        }),
        json!([rule(
            "describe",
            json!("aws"),
            json!({"kind":"any"}),
            "allow"
        )]),
        json!([
            gate(
                "required-profile",
                "profile",
                json!({"var":"profile"}),
                "Use --profile {{profile}}."
            ),
            gate(
                "blocked-profiles",
                "blocked_profiles",
                json!({"not":{"var":"blocked_profiles"}}),
                "Blocked AWS profiles: {{blocked_profiles}}."
            )
        ]),
    )
}

#[test]
fn profile_gates_scan_global_options_and_block_listed_values() {
    let source = profile_gate_source();
    let ec2 = [
        "aws",
        "ec2",
        "describe-instances",
        "--filters",
        "Name=x,Values=y",
    ];
    let with = |extra: &[&'static str]| [&ec2[..], extra].concat();
    let open = Pack::from_yaml(&source).unwrap();
    assert_eq!(decide(&open, &ec2), Decision::Allow);

    let required =
        Pack::from_yaml_bound(&source, &bound(json!({"profile":"acme-ro"})), true).unwrap();
    for (extra, expected) in [
        (&[][..], Decision::Deny),
        (&["--profile", "acme-ro"], Decision::Allow),
        (&["--prof=acme-ro"], Decision::Allow),
        (&["--profile", "acme-admin"], Decision::Deny),
        (
            &["--profile", "acme-ro", "--p", "acme-admin"],
            Decision::Deny,
        ),
    ] {
        assert_eq!(decide(&required, &with(extra)), expected, "{extra:?}");
    }

    let blocked = Pack::from_yaml_bound(
        &source,
        &bound(json!({"blocked_profiles":["acme-prod","acme-admin"]})),
        true,
    )
    .unwrap();
    for (extra, expected) in [
        (&[][..], Decision::Deny),
        (&["--profile", "acme-dev"], Decision::Allow),
        (&["--pro", "acme-prod"], Decision::Deny),
        (
            &["--profile", "acme-dev", "--profile=acme-admin"],
            Decision::Deny,
        ),
    ] {
        assert_eq!(decide(&blocked, &with(extra)), expected, "{extra:?}");
    }
    let refused = serde_json::to_value(
        evaluate(
            std::slice::from_ref(&blocked),
            None,
            Profile::Recommended,
            &request(&with(&["--profile", "acme-prod"])),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        refused["guidance"][0]["message"],
        "Blocked AWS profiles: acme-prod, acme-admin."
    );
    // The same gate, compiled, decides identically.
    let plan = Plan::compile(vec![(blocked, Profile::Recommended)], None, json!({})).unwrap();
    let plan = Plan::load(plan.bytes().to_vec()).unwrap();
    for (extra, expected) in [
        (&["--profile", "acme-dev"][..], Decision::Allow),
        (&["--profile", "acme-prod"], Decision::Deny),
    ] {
        assert_eq!(
            plan.evaluate(&request(&with(extra)), &|| Ok(()), true, true)
                .unwrap()
                .decision,
            expected
        );
    }
}

#[test]
fn compiled_plans_carry_preconditions_and_resolved_rules() {
    let pack = Pack::from_yaml_bound(
        &node_pack(),
        &bound(json!({"package_manager":"yarn"})),
        true,
    )
    .unwrap();
    let plan = Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    let plan = Plan::load(plan.bytes().to_vec()).unwrap();
    for (command, expected) in [
        (&["npm", "install"][..], Decision::Deny),
        (&["yarn", "install"], Decision::Allow),
        (&["npx", "acme"], Decision::Allow),
    ] {
        assert_eq!(
            plan.evaluate(&request(command), &|| Ok(()), true, true)
                .unwrap()
                .decision,
            expected,
            "{command:?}"
        );
    }
}

#[test]
fn lists_paths_and_tests_resolve_by_type() {
    let source = document(
        &[
            "structured-rules-v1",
            "argument-rules-v1",
            "location-v1",
            "variables-v1",
        ],
        json!({
            "tools":{"type":"list","description":"Build tools","items":{"type":"string","max_length":32},"max_items":4},
            "scratch":{"type":"path","description":"Scratch folder","default":"tmp/scratch"},
            "strict":{"type":"boolean","description":"Ask for every tool","default":false}
        }),
        json!([
            rule("tools", json!({"var":"tools"}), json!({"kind":"any"}), "allow"),
            {"id":"strict-tools","when":[{"var":"strict","equals":true},{"var":"tools","contains":"make"}],"decision":"ask","conditions":argv("make", json!({"kind":"prefix","values":["install"]}))},
            {"id":"clean","decision":"deny","conditions":{"domain":"argv","executable":"rm","arguments":{"kind":"any"},"cwd":{"within":{"var":"scratch"}}}}
        ]),
        Value::Null,
    );
    let unset = Pack::from_yaml(&source).unwrap();
    assert_eq!(decide(&unset, &["make"]), Decision::Ask);
    let tools =
        Pack::from_yaml_bound(&source, &bound(json!({"tools":["make","cargo"]})), true).unwrap();
    assert_eq!(decide(&tools, &["make"]), Decision::Allow);
    assert_eq!(decide(&tools, &["cargo", "build"]), Decision::Allow);
    assert_eq!(decide(&tools, &["make", "install"]), Decision::Allow);
    let strict = Pack::from_yaml_bound(
        &source,
        &bound(json!({"tools":["make"],"strict":true})),
        true,
    )
    .unwrap();
    assert_eq!(decide(&strict, &["make", "install"]), Decision::Ask);
    let entry = strict
        .resolution()
        .iter()
        .find(|e| e.id == "clean")
        .unwrap();
    assert_eq!(entry.variables["scratch"], json!("tmp/scratch"));
    for invalid in [
        json!({"tools":[]}),
        json!({"tools":["make","make"]}),
        json!({"tools":["a","b","c","d","e"]}),
        json!({"tools":"make"}),
        json!({"scratch":"../outside"}),
        json!({"scratch":"/absolute"}),
        json!({"strict":"yes"}),
        json!({"unknown":"value"}),
    ] {
        assert!(
            Pack::from_yaml_bound(&source, &bound(invalid.clone()), true).is_err(),
            "{invalid}"
        );
    }
    // An oversized path refuses as a named binding, without the value.
    let long = format!("tmp/{}", "a".repeat(4096));
    match Pack::from_yaml_bound(&source, &bound(json!({"scratch":long})), true) {
        Err(error @ super::Error::Binding { .. }) => {
            let super::Error::Binding {
                variable, reason, ..
            } = &error
            else {
                unreachable!()
            };
            assert_eq!(
                (variable.as_str(), *reason),
                ("scratch", "variable value is too long")
            );
            assert!(!error.to_string().contains("aaaa"));
        }
        Err(other) => panic!("unexpected refusal: {other}"),
        Ok(_) => panic!("oversized path accepted"),
    }
}

#[test]
fn environment_variable_names_are_not_variable_references() {
    let environment = |id: &str, fields: Value| {
        let mut conditions = argv("acme", json!({"kind":"exact","values":[id]}));
        conditions["environment"] = json!({"fields":fields});
        json!({"id":id,"decision":"deny","conditions":conditions})
    };
    let source = document(
        &["structured-rules-v1", "argument-rules-v1", "variables-v1"],
        json!({"zone":{"type":"string","description":"Zone"}}),
        json!([
            environment("literal", json!({"var":"zone"})),
            environment("selected", json!({"ZONE":{"var":"zone"}})),
        ]),
        Value::Null,
    );
    let states = |pack: &Pack| -> Vec<(String, &'static str)> {
        pack.resolution()
            .iter()
            .map(|e| (e.id.clone(), e.state))
            .collect()
    };
    // Only entries that reference variables are listed: `literal` names none.
    let unset = Pack::from_yaml(&source).unwrap();
    assert_eq!(states(&unset), [("selected".to_owned(), "inactive")]);
    let bound = Pack::from_yaml_bound(&source, &bound(json!({"zone":"eu"})), true).unwrap();
    assert_eq!(states(&bound), [("selected".to_owned(), "active")]);
}

#[test]
fn declarations_references_and_capabilities_are_validated() {
    let caps = &["structured-rules-v1", "argument-rules-v1", "variables-v1"];
    let tool = json!({"tool":{"type":"string","description":"Tool"}});
    let uses = json!([rule(
        "tool",
        json!({"var":"tool"}),
        json!({"kind":"any"}),
        "allow"
    )]);
    assert!(Pack::from_yaml(&document(caps, tool.clone(), uses.clone(), Value::Null)).is_ok());
    for (capabilities, variables, rules) in [
        (
            &["structured-rules-v1", "argument-rules-v1"][..],
            tool.clone(),
            uses.clone(),
        ),
        (caps, json!({}), uses.clone()),
        (
            caps,
            json!({"tool":{"type":"boolean","description":"Tool"}}),
            uses.clone(),
        ),
        (
            caps,
            json!({"Tool":{"type":"string","description":"Tool"}}),
            json!([]),
        ),
        (
            caps,
            json!({"tool":{"type":"string","description":""}}),
            json!([]),
        ),
        (
            caps,
            json!({"tool":{"type":"choice","description":"Tool"}}),
            json!([]),
        ),
        (
            caps,
            json!({"tool":{"type":"choice","description":"Tool","values":["a"],"default":"b"}}),
            json!([]),
        ),
        (
            caps,
            json!({"tool":{"type":"string","description":"Tool","items":{"type":"string"}}}),
            json!([]),
        ),
        (
            caps,
            json!({"tool":{"type":"list","description":"Tool","items":{"type":"boolean"}}}),
            json!([]),
        ),
        (
            caps,
            tool.clone(),
            json!([rule(
                "tool",
                json!({"exact":{"var":"tool"}}),
                json!({"kind":"any"}),
                "allow"
            )]),
        ),
        (
            caps,
            json!({"tool":{"type":"list","description":"Tool","items":{"type":"string"}}}),
            json!([rule(
                "tool",
                json!("acme"),
                json!({"kind":"exact","values":[{"var":"tool"}]}),
                "allow"
            )]),
        ),
        (
            caps,
            tool.clone(),
            json!([{"id":"t","when":[{"var":"tool","contains":"x"}],"decision":"ask","conditions":argv("acme", json!({"kind":"any"}))}]),
        ),
        (
            caps,
            tool.clone(),
            json!([{"id":"t","when":[{"var":"tool","set":true,"equals":"x"}],"decision":"ask","conditions":argv("acme", json!({"kind":"any"}))}]),
        ),
    ] {
        assert!(
            Pack::from_yaml(&document(
                capabilities,
                variables.clone(),
                rules.clone(),
                Value::Null
            ))
            .is_err(),
            "{variables} {rules}"
        );
    }
    let required = json!({"tool":{"type":"string","description":"Tool","required":true}});
    let source = document(caps, required, uses, Value::Null);
    assert!(Pack::from_yaml(&source).is_ok());
    assert!(Pack::from_yaml_bound(&source, &Binding::default(), true).is_err());
    assert!(Pack::from_yaml_bound(&source, &bound(json!({"tool":"make"})), true).is_ok());
}

#[test]
fn binding_refusals_name_the_variable_and_reason_but_not_the_value() {
    let source = node_pack();
    let refusal =
        |binding: &Binding, required: bool| match Pack::from_yaml_bound(&source, binding, required)
        {
            Err(super::Error::Binding {
                selection,
                variable,
                reason,
            }) => (selection, variable, reason),
            Err(other) => panic!("unexpected refusal: {other}"),
            Ok(_) => panic!("binding accepted"),
        };
    let (selection, variable, reason) =
        refusal(&bound(json!({"package_manager":"acme-bun"})), false);
    assert_eq!(
        (selection.as_str(), variable.as_str()),
        ("", "package_manager")
    );
    assert_ne!(reason, "");
    let (_, variable, reason) = refusal(&bound(json!({"runtime":"node"})), false);
    assert_eq!(variable, "runtime");
    assert_eq!(reason, "binding names an undeclared variable");
    let required = document(
        &["structured-rules-v1", "argument-rules-v1", "variables-v1"],
        json!({"tool":{"type":"string","description":"Tool","required":true}}),
        json!([rule(
            "tool",
            json!({"var":"tool"}),
            json!({"kind":"any"}),
            "allow"
        )]),
        Value::Null,
    );
    match Pack::from_yaml_bound(&required, &Binding::default(), true) {
        Err(super::Error::Binding {
            variable, reason, ..
        }) => {
            assert_eq!(variable, "tool");
            assert_eq!(reason, "a required pack variable is unset");
        }
        _ => panic!("unset required variable accepted"),
    }
    let message = super::Error::binding("package_manager", "a required pack variable is unset")
        .in_selection("acme/tools")
        .to_string();
    assert!(
        message.contains("pack acme/tools variable package_manager"),
        "{message}"
    );
    assert!(
        !refusal(&bound(json!({"package_manager":"acme-bun"})), false)
            .2
            .contains("acme-bun")
    );
    let long = "x".repeat(200);
    let bounded = super::Error::binding(&long, "reason").in_selection(&long);
    let super::Error::Binding {
        selection,
        variable,
        ..
    } = bounded
    else {
        unreachable!()
    };
    assert!(selection.chars().count() <= 64 && variable.chars().count() <= 64);
}

#[test]
fn templates_render_bounded_text_only_with_their_capability() {
    let caps = &[
        "structured-rules-v1",
        "argument-rules-v1",
        "variables-v1",
        "templates-v1",
    ];
    let variables =
        json!({"name":{"type":"string","description":"Name","max_length":1024,"default":"acme"}});
    let guided = |message: &str| json!([{"id":"guided","decision":"deny","guidance":{"message":message},"conditions":argv("acme", json!({"kind":"any"}))}]);
    let message = |pack: &Pack| {
        let evaluation = evaluate(
            std::slice::from_ref(pack),
            None,
            Profile::Recommended,
            &request(&["acme"]),
        )
        .unwrap();
        serde_json::to_value(evaluation).unwrap()["guidance"][0]["message"].clone()
    };
    let pack = Pack::from_yaml(&document(
        caps,
        variables.clone(),
        guided("Use {{name}} {here}."),
        Value::Null,
    ))
    .unwrap();
    assert_eq!(message(&pack), "Use acme {here}.");
    let literal = Pack::from_yaml(&document(
        &["structured-rules-v1", "argument-rules-v1", "variables-v1"],
        variables.clone(),
        guided("Use {{name}}."),
        Value::Null,
    ))
    .unwrap();
    assert_eq!(message(&literal), "Use {{name}}.");
    for text in [
        "Use {{Name}}.",
        "Use {{name}.",
        "Use }} here.",
        "Use {{other}}.",
        "No placeholders.",
    ] {
        assert!(
            Pack::from_yaml(&document(
                caps,
                variables.clone(),
                guided(text),
                Value::Null
            ))
            .is_err(),
            "{text}"
        );
    }
    let long = "x".repeat(600);
    let source = document(caps, variables, guided("Use {{name}}."), Value::Null);
    assert!(Pack::from_yaml_bound(&source, &bound(json!({"name":long})), true).is_err());
}

#[test]
fn overrides_change_or_remove_entries_without_new_conditions() {
    let source = node_pack();
    let overrides = |entries: Value| Binding {
        variables: serde_json::from_value(json!({"package_manager":"pnpm"})).unwrap(),
        overrides: serde_json::from_value(entries).unwrap(),
    };
    let off =
        Pack::from_yaml_bound(&source, &overrides(json!({"package-manager":"off"})), true).unwrap();
    assert_eq!(decide(&off, &["npm", "install"]), Decision::Allow);
    assert_eq!(
        off.resolution()
            .iter()
            .find(|e| e.id == "package-manager")
            .unwrap()
            .overridden,
        Some(Override::Off)
    );
    let ask = Pack::from_yaml_bound(
        &source,
        &overrides(json!({"package-manager":{"on_unmet":"ask"},"install":"deny"})),
        true,
    )
    .unwrap();
    assert_eq!(decide(&ask, &["npm", "install"]), Decision::Ask);
    assert_eq!(decide(&ask, &["pnpm", "install"]), Decision::Deny);
    for invalid in [
        json!({"missing":"off"}),
        json!({"install":"maybe"}),
        json!({"install":{}}),
        json!({"install":{"on_unmet":"ask"}}),
        json!({"package-manager":"allow"}),
        json!({"package-manager":{"on_unmet":"allow"}}),
    ] {
        let binding =
            serde_json::from_value::<BTreeMap<String, Override>>(invalid.clone()).map(|entries| {
                Binding {
                    variables: BTreeMap::new(),
                    overrides: entries,
                }
            });
        assert!(
            binding.map_or(true, |b| Pack::from_yaml_bound(&source, &b, true).is_err()),
            "{invalid}"
        );
    }
}

#[test]
fn static_packs_keep_their_artifact_digest_and_resolve_to_themselves() {
    let source = document(
        &["structured-rules-v1", "argument-rules-v1"],
        Value::Null,
        json!([rule("make", json!("make"), json!({"kind":"any"}), "allow")]),
        Value::Null,
    );
    let pack = Pack::from_yaml(&source).unwrap();
    assert_eq!(pack.digest, super::super::fingerprint(source.as_bytes()));
    assert!(pack.resolution().is_empty());
    let plain = Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    let bound = Plan::compile(
        vec![(
            Pack::from_yaml_bound(&source, &Binding::default(), true).unwrap(),
            Profile::Recommended,
        )],
        None,
        json!({}),
    )
    .unwrap();
    assert_eq!(plain.bytes(), bound.bytes());
}

/// A raw pack source whose `variables` keys keep the given order.
fn ordered(variables: &str) -> String {
    format!(
        r#"{{"schema_version":1,"kind":"pack","id":"acme/cloud","version":"0.1.0","description":"Acme cloud","license":"MIT","maintainers":["Acme"],"engines":{{"shield":"*"}},"capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],"variables":{{{variables}}},"rules":[{{"id":"cloud","decision":"allow","conditions":{{"domain":"argv","executable":"acme-cloud","arguments":{{"kind":"sequence","prefix":[{{"var":"profile"}}],"rest":{{"kind":"any"}}}}}}}}]}}"#
    )
}

#[test]
fn setup_wording_and_ask_when_are_validated_in_declaration_order() {
    let profile = r#""profile":{"type":"string","description":"Profile","setup":{"question":"Restrict the profile?","unset":"Any profile","value":"Only this profile","placeholder":"acme-read-only"}}"#;
    let endpoint = r#""endpoint":{"type":"string","description":"Endpoint","setup":{"ask_when":[{"var":"profile","set":true}]}}"#;
    let pack = ordered(&format!("{profile},{endpoint}"));
    Pack::from_yaml(&pack).unwrap();
    let names: Vec<_> = super::super::ordered_declarations(&pack)
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        ["profile", "endpoint"],
        "declaration order, not by name"
    );
    let refused = [
        // ask_when on a variable declared later.
        format!("{endpoint},{profile}"),
        // Typed-row wording on a choice variable.
        format!(
            r#"{profile},"zone":{{"type":"choice","description":"Zone","values":["a"],"setup":{{"placeholder":"a"}}}}"#
        ),
        // A required variable could be skipped.
        format!(
            r#"{profile},"zone":{{"type":"string","description":"Zone","required":true,"setup":{{"ask_when":[{{"var":"profile","set":true}}]}}}}"#
        ),
        // A required variable has no unset answer.
        r#""profile":{"type":"string","description":"Profile","required":true,"setup":{"unset":"Any"}}"#.to_owned(),
        // Wording limits and unknown fields.
        format!(r#""profile":{{"type":"string","description":"P","setup":{{"value":"{}"}}}}"#, "x".repeat(65)),
        r#""profile":{"type":"string","description":"P","setup":{"label":"x"}}"#.to_owned(),
        // A question cannot depend on itself.
        r#""profile":{"type":"string","description":"P","setup":{"ask_when":[{"var":"profile","set":true}]}}"#.to_owned(),
    ];
    for variables in refused {
        assert!(
            Pack::from_yaml(&ordered(&variables)).is_err(),
            "{variables}"
        );
    }
}

const LOGS_CAPABILITIES: &[&str] = &[
    "structured-rules-v1",
    "argument-rules-v1",
    "requirements-v1",
    "variables-v1",
    "agent-outputs-v1",
];

/// One rule that allows any profile and region until each is set.
fn logs_pack(requirements: &Value) -> String {
    let grammar = |values: Value| json!({"kind":"options","leading":{"flags":[],"values":values.clone()},"prefix":["logs","filter"],"flags":[],"values":values,"operands":false});
    let mut required = grammar(json!([
        {"name":"--profile","match":{"var":"profile","unset":"any"}},
        {"name":"--region","match":{"var":"region","unset":"any"}}
    ]));
    for (key, value) in requirements.as_object().unwrap() {
        required[key] = value.clone();
    }
    document(
        LOGS_CAPABILITIES,
        json!({"profile":{"type":"string","description":"Profile"},"region":{"type":"string","description":"Region"}}),
        json!([{"id":"logs-filter","decision":"allow","agents":{"codex":{"prefixes":[["acme","logs","filter"]]}},
            "conditions":argv("acme", grammar(json!(["--profile","--region"]))),
            "requirements":{"arguments":required},"on_unmet":{"decision":"deny"}}]),
        Value::Null,
    )
}

fn optional_pack() -> String {
    logs_pack(&json!({"require_all":[
        {"name":"--profile","when":[{"var":"profile","set":true}]},
        {"name":"--region","when":[{"var":"region","set":true}]}
    ]}))
}

#[test]
fn optional_checks_tighten_one_rule_as_variables_are_set() {
    let source = optional_pack();
    let none = Pack::from_yaml(&source).unwrap();
    for command in [
        &["acme", "logs", "filter"][..],
        &["acme", "logs", "filter", "--profile", "other"],
        &["acme", "--region", "eu", "logs", "filter"],
    ] {
        assert_eq!(decide(&none, command), Decision::Allow, "{command:?}");
    }
    assert_eq!(
        decide(&none, &["acme", "logs", "filter", "--debug"]),
        Decision::Ask
    );

    let profile =
        Pack::from_yaml_bound(&source, &bound(json!({"profile":"acme-ro"})), true).unwrap();
    for (command, expected) in [
        (
            &["acme", "logs", "filter", "--profile", "acme-ro"][..],
            Decision::Allow,
        ),
        (
            &[
                "acme",
                "--profile",
                "acme-ro",
                "logs",
                "filter",
                "--region",
                "eu",
            ],
            Decision::Allow,
        ),
        (&["acme", "logs", "filter"], Decision::Deny),
        (
            &["acme", "logs", "filter", "--profile", "acme-admin"],
            Decision::Deny,
        ),
    ] {
        assert_eq!(decide(&profile, command), expected, "{command:?}");
    }

    let both = Pack::from_yaml_bound(
        &source,
        &bound(json!({"profile":"acme-ro","region":"us"})),
        true,
    )
    .unwrap();
    for (command, expected) in [
        (
            &[
                "acme",
                "logs",
                "filter",
                "--profile",
                "acme-ro",
                "--region",
                "us",
            ][..],
            Decision::Allow,
        ),
        (
            &["acme", "logs", "filter", "--profile", "acme-ro"],
            Decision::Deny,
        ),
        (
            &[
                "acme",
                "logs",
                "filter",
                "--profile",
                "acme-ro",
                "--region",
                "eu",
            ],
            Decision::Deny,
        ),
    ] {
        assert_eq!(decide(&both, command), expected, "{command:?}");
    }
    let entry = both
        .resolution()
        .iter()
        .find(|e| e.id == "logs-filter")
        .unwrap();
    assert_eq!(entry.state, "active");
    assert_eq!(entry.variables.len(), 2);
}

#[test]
fn optional_checks_apply_to_forbidden_options_and_preconditions() {
    // --region is refused until the region variable is set, then must match it.
    let forbid =
        logs_pack(&json!({"forbid":[{"name":"--region","when":[{"var":"region","set":false}]}]}));
    let unset = Pack::from_yaml(&forbid).unwrap();
    assert_eq!(decide(&unset, &["acme", "logs", "filter"]), Decision::Allow);
    assert_eq!(
        decide(&unset, &["acme", "logs", "filter", "--region", "eu"]),
        Decision::Deny
    );
    let region = Pack::from_yaml_bound(&forbid, &bound(json!({"region":"eu"})), true).unwrap();
    assert_eq!(
        decide(&region, &["acme", "logs", "filter", "--region", "eu"]),
        Decision::Allow
    );
    assert_eq!(
        decide(&region, &["acme", "logs", "filter", "--region", "us"]),
        Decision::Deny
    );

    // The same forms inside a precondition's argument grammar.
    let source = document(
        &[
            "structured-rules-v1",
            "argument-rules-v1",
            "variables-v1",
            "preconditions-v1",
        ],
        json!({"profile":{"type":"string","description":"Profile"}}),
        json!([rule("any", json!("acme"), json!({"kind":"any"}), "allow")]),
        json!([{"id":"aws-profile","conditions":argv("acme", json!({"kind":"any"})),
            "requires_any":[{"arguments":{"kind":"options","prefix":[],"flags":[],
                "values":[{"name":"--profile","match":{"var":"profile","unset":"any"}}],"operands":true,
                "require_all":[{"name":"--profile","when":[{"var":"profile","set":true}]}]}}],
            "on_unmet":{"decision":"deny"}}]),
    );
    let unset = Pack::from_yaml(&source).unwrap();
    assert_eq!(decide(&unset, &["acme", "logs"]), Decision::Allow);
    let profile =
        Pack::from_yaml_bound(&source, &bound(json!({"profile":"acme-ro"})), true).unwrap();
    assert_eq!(decide(&profile, &["acme", "logs"]), Decision::Deny);
    assert_eq!(
        decide(&profile, &["acme", "--profile", "acme-ro", "logs"]),
        Decision::Allow
    );
}

#[test]
fn optional_check_forms_refuse_outside_option_values() {
    let invalid = [
        // Only `any` is a valid unset behavior.
        optional_pack().replace(r#""unset":"any""#, r#""unset":"deny""#),
        // A conditional item has exactly name and when.
        logs_pack(
            &json!({"require_all":[{"name":"--profile","when":[{"var":"profile","set":true}],"extra":1}]}),
        ),
        logs_pack(&json!({"require_all":[{"name":"--profile"}]})),
        // Item tests name declared variables of the same pack.
        logs_pack(
            &json!({"require_all":[{"name":"--profile","when":[{"var":"missing","set":true}]}]}),
        ),
        // `unset` belongs only to an option value's match.
        document(
            LOGS_CAPABILITIES,
            json!({"tool":{"type":"string","description":"Tool"}}),
            json!([{"id":"tool","decision":"allow",
                "conditions":argv("acme", json!({"kind":"prefix","values":[{"var":"tool","unset":"any"}]}))}]),
            Value::Null,
        ),
    ];
    for source in invalid {
        assert!(Pack::from_yaml(&source).is_err(), "{source}");
    }
    // An emptied list imposes no presence condition.
    let source = logs_pack(
        &json!({"require_any":[{"name":"--profile","when":[{"var":"profile","set":true}]}]}),
    );
    let pack = Pack::from_yaml(&source).unwrap();
    assert_eq!(decide(&pack, &["acme", "logs", "filter"]), Decision::Allow);
}

#[test]
fn compiled_plans_list_codex_prefixes_of_active_allowing_rules() {
    let compile = |binding: Binding| {
        let pack = Pack::from_yaml_bound(&optional_pack(), &binding, true).unwrap();
        Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap()
    };
    let plan = compile(bound(json!({"profile":"acme-ro"})));
    let plan = Plan::load(plan.bytes().to_vec()).unwrap();
    let prefixes = plan.codex_prefixes();
    assert_eq!(prefixes.len(), 1);
    assert_eq!(prefixes[0].owner, "acme/tools");
    assert_eq!(prefixes[0].rule, "logs-filter");
    assert_eq!(
        prefixes[0].pattern,
        vec![vec!["acme"], vec!["logs"], vec!["filter"]]
    );

    for overrides in [json!({"logs-filter":"off"}), json!({"logs-filter":"ask"})] {
        let plan = compile(Binding {
            variables: BTreeMap::new(),
            overrides: serde_json::from_value(overrides).unwrap(),
        });
        assert_eq!(plan.codex_prefixes(), []);
    }
    let missing = optional_pack().replace(r#""agent-outputs-v1""#, r#""templates-v1""#);
    assert!(Pack::from_yaml(&missing).is_err());
}

#[test]
fn rules_without_messages_name_the_failed_option_for_agents() {
    let pack = Pack::from_yaml_bound(&optional_pack(), &bound(json!({"profile":"acme-ro"})), true)
        .unwrap();
    let message = |command: &[&str]| {
        let evaluation = evaluate(
            std::slice::from_ref(&pack),
            None,
            Profile::Recommended,
            &request(command),
        )
        .unwrap();
        crate::agent_message::detail(&serde_json::to_value(evaluation).unwrap()).unwrap()
    };
    assert!(
        message(&["acme", "logs", "filter", "--profile", "acme-admin"])
            .starts_with("--profile must be acme-ro.")
    );
    assert!(
        message(&["acme", "logs", "filter"]).starts_with("Missing required option: --profile.")
    );
}
