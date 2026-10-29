//! Flat-reference behavior, reversible provenance and non-expanding preparation.
use super::*;
use crate::policy::{self, Pack, Profile, compiled::Plan};

fn document() -> Value {
    json!({"schema_version":1,"kind":"pack","id":"acme/shared","version":"0.1.0","description":"Acme shared commands","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
    "capabilities":["structured-rules-v1","argument-rules-v1","invocation-groups-v1","command-sets-v1","requirements-v1","glob-v1"],
    "invocations":{"globals":{"id_prefix":"w","segments":[[[],["--plain"]],[[],["--color"]]]},"versions":{"id_prefix":"v","segments":[[[],["--plain"]],[[],["--color"]]]}},
    "command_sets":{"inspection":{"status":"base-status","diff":"base-diff"}},
    "rules":[
        {"id":"inspection","invocation":"globals","command_set":"inspection","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"decision":"allow","reason":"Inspect Acme"},
        {"id":"publish","invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["publish"]}},"decision":"ask"},
        {"id":"dry","invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"options","prefix":["publish"],"flags":["--dry-run"],"values":[],"operands":false}},"requirements":{"arguments":{"kind":"exact","values":["publish","--dry-run"]}},"on_unmet":{"decision":"deny","message":"Use the exact inspected request","alternatives":["acme publish --dry-run"]},"decision":"allow"},
        {"id":"force","invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"risk_tokens","prefix":["publish"],"any_of":["--force"]}},"decision":{"loose":"ask","recommended":"deny","strict":"deny"}},
        {"id":"report","invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"sequence","prefix":["show",{"glob":"*.txt"}],"rest":{"kind":"none"}}},"decision":"allow"},
        {"id":"version","invocation":"versions","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["--version"]}},"decision":"allow"},
        {"id":"ordinary","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["ordinary"]}},"decision":"allow"}
    ]})
}

fn expand(source: &Value) -> Value {
    let mut flat = source.clone();
    let mut rows = Vec::new();
    for body in source["rules"].as_array().unwrap() {
        let Some(name) = body["invocation"].as_str() else {
            rows.push(body.clone());
            continue;
        };
        let group = &source["invocations"][name];
        let mut prefixes = vec![vec![]];
        for segment in group["segments"].as_array().unwrap() {
            prefixes = prefixes
                .iter()
                .flat_map(|prefix| {
                    segment.as_array().unwrap().iter().map(move |choice| {
                        let mut vector = prefix.clone();
                        vector.extend(choice.as_array().unwrap().clone());
                        vector
                    })
                })
                .collect();
        }
        let members = body["command_set"]
            .as_str()
            .map(|set| source["command_sets"][set].as_object().unwrap());
        for (ordinal, prefix) in prefixes.iter().enumerate() {
            let cases: Vec<_> = members.map_or_else(
                || vec![(None, body["id"].clone())],
                |members| {
                    members
                        .iter()
                        .map(|(command, id)| (Some(command), id.clone()))
                        .collect()
                },
            );
            for (command, id) in cases {
                let mut row = body.clone();
                row.as_object_mut().unwrap().remove("invocation");
                row.as_object_mut().unwrap().remove("command_set");
                row["id"] = json!(format!(
                    "{}{ordinal}-{}",
                    group["id_prefix"].as_str().unwrap(),
                    id.as_str().unwrap()
                ));
                let mut vector = prefix.clone();
                if let Some(command) = command {
                    vector.push(json!(command));
                }
                let args = &mut row["conditions"]["arguments"];
                if args["kind"] == "any" {
                    if !vector.is_empty() {
                        *args = json!({"kind":"prefix","values":vector});
                    }
                } else {
                    let field = if args.get("values").is_some()
                        && matches!(args["kind"].as_str(), Some("exact" | "prefix"))
                    {
                        "values"
                    } else {
                        "prefix"
                    };
                    vector.extend(args[field].as_array().unwrap().clone());
                    args[field] = json!(vector);
                }
                rows.push(row);
            }
        }
    }
    flat["rules"] = json!(rows);
    flat.as_object_mut().unwrap().remove("invocations");
    flat.as_object_mut().unwrap().remove("command_sets");
    flat["capabilities"]
        .as_array_mut()
        .unwrap()
        .retain(|v| !["invocation-groups-v1", "command-sets-v1"].contains(&v.as_str().unwrap()));
    flat
}

fn request(args: &[&str]) -> Request {
    Request {
        executable: "acme".into(),
        arguments: args.iter().map(|s| (*s).into()).collect(),
        cwd: "/acme".into(),
        project: "/acme".into(),
    }
}

fn evidence(evaluation: policy::Evaluation) -> Value {
    let mut value = serde_json::to_value(evaluation).unwrap();
    value.as_object_mut().unwrap().remove("revision");
    for pack in value["packs"].as_array_mut().unwrap() {
        pack.as_object_mut().unwrap().remove("digest");
    }
    value
}

#[test]
fn shared_routes_preserve_complete_flat_results_and_inspectable_identities() {
    let source = document();
    let flat = expand(&source);
    for profile in [Profile::Loose, Profile::Recommended, Profile::Strict] {
        let shared = Pack::from_yaml(&source.to_string()).unwrap();
        let original = Pack::from_yaml(&flat.to_string()).unwrap();
        let plan = Plan::compile(
            vec![(Pack::from_yaml(&source.to_string()).unwrap(), profile)],
            None,
            json!({}),
        )
        .unwrap();
        for args in [
            vec!["status"],
            vec!["diff", "ordinary.txt"],
            vec!["--plain", "status"],
            vec!["--color", "diff"],
            vec!["--plain", "--color", "status"],
            vec!["--color", "--plain", "status"],
            vec!["--plain", "--plain", "status"],
            vec!["--unknown", "status"],
            vec!["inspection"],
            vec!["publish", "--dry-run"],
            vec!["--plain", "publish", "--dry-run"],
            vec!["publish", "--force"],
            vec!["--plain", "publish", "--force"],
            vec!["show", "report.txt"],
            vec!["show", "report.bin"],
            vec!["--plain", "show", "report.txt"],
            vec!["--version"],
            vec!["--plain", "--version"],
            vec!["ordinary"],
            vec![],
        ] {
            let request = request(&args);
            let expected = evidence(
                policy::evaluate_selected(&[(&original, profile)], &[], &request).unwrap(),
            );
            let actual =
                evidence(policy::evaluate_selected(&[(&shared, profile)], &[], &request).unwrap());
            assert_eq!(actual, expected, "source {profile:?} {args:?}");
            assert_eq!(
                evidence(plan.evaluate(&request, &|| Ok(()), true, true).unwrap()),
                expected,
                "compiled {profile:?} {args:?}"
            );
            for matched in actual["matches"].as_array().unwrap() {
                let id = matched["rule_id"].as_str().unwrap();
                let inspected = policy::inspect_pack_rule(&source.to_string(), id)
                    .unwrap()
                    .unwrap();
                let original = flat["rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == id)
                    .unwrap();
                assert_eq!(&inspected, original, "lookup {id}");
            }
        }
    }
    for id in [
        "w0-inspection",
        "w4-base-status",
        "w00-base-status",
        "w0-unknown",
    ] {
        assert!(
            policy::inspect_pack_rule(&source.to_string(), id)
                .unwrap()
                .is_none(),
            "{id}"
        );
    }
}

#[test]
fn ambiguous_routes_retain_distinct_matches_and_cancel_without_partial_decisions() {
    let mut source = document();
    source["invocations"]["globals"]["segments"] = json!([[[], ["x"]], [[], ["x"]]]);
    let shared = Pack::from_yaml(&source.to_string()).unwrap();
    let result = policy::evaluate_selected(
        &[(&shared, Profile::Recommended)],
        &[],
        &request(&["x", "status"]),
    )
    .unwrap();
    let ids: BTreeSet<_> = result.matches.iter().map(|m| m.rule_id.as_str()).collect();
    assert_eq!(ids, BTreeSet::from(["w1-base-status", "w2-base-status"]));
    assert!(matches!(
        routes(
            &vec![vec![vec![], vec!["x".into()]]; 16],
            &vec!["x".into(); 16],
            &|| Ok(())
        ),
        Err(Error::Limit)
    ));
    assert!(matches!(
        routes(&vec![vec![vec![]]], &[], &|| Err(Error::Interrupted)),
        Err(Error::Interrupted)
    ));
}

#[test]
fn unrelated_executable_does_not_parse_an_ambiguous_invocation_language() {
    let mut source = document();
    source["invocations"]["globals"]["segments"] = json!(vec![vec![vec![], vec!["x"]]; 16]);
    let shared = Pack::from_yaml(&source.to_string()).unwrap();
    let mut request = request(&["x"; 16]);
    request.executable = "other".into();
    for indexed in [true, false] {
        assert_eq!(
            policy::evaluate_selected_with(
                &[(&shared, Profile::Recommended)],
                &[],
                &request,
                indexed
            )
            .unwrap()
            .decision,
            policy::Decision::Ask
        );
    }
    let plan = Plan::compile(vec![(shared, Profile::Recommended)], None, json!({})).unwrap();
    assert_eq!(
        plan.evaluate(&request, &|| Ok(()), true, true)
            .unwrap()
            .decision,
        policy::Decision::Ask
    );
}

#[test]
fn changed_invocation_languages_cannot_receive_a_canonical_body_equivalence_proof() {
    use crate::configuration::{Candidate, Relation, compare_policy};
    let source = document();
    let mut changed = source.clone();
    changed["invocations"]["globals"]["segments"] = json!([[[], ["--other"]]]);
    let capture = |doc: &Value| {
        let config=json!({"schema_version":1,"packs":[{"id":"acme/shared","version":"0.1.0","profile":"recommended","path":"pack.json"}]}).to_string();
        (
            policy::Scope::Project,
            Candidate::from_yaml(&config)
                .unwrap()
                .capture(&[doc.to_string()], None)
                .unwrap(),
        )
    };
    let first = [capture(&source)];
    let second = [capture(&changed)];
    let report = compare_policy(&first, &second).unwrap();
    assert!(!report.complete);
    assert_eq!(report.relation, Relation::Unknown);
    let identity = compare_policy(&first, &first).unwrap();
    assert!(identity.complete);
    assert_eq!(identity.relation, Relation::Equivalent);
}

#[test]
fn invalid_bindings_capabilities_and_identity_collisions_are_refused() {
    let original = document();
    let mut changes = Vec::new();
    for (pointer, value) in [
        ("/rules/0/invocation", json!("unknown")),
        ("/rules/0/invocation", Value::Null),
        ("/rules/0/command_set", json!("unknown")),
        ("/invocations/globals/id_prefix", json!("w-bad")),
        ("/invocations/globals/segments", json!([[[], []]])),
        (
            "/invocations/globals/segments",
            json!([[["--api-key=private"]]]),
        ),
        (
            "/invocations/globals/segments",
            json!([[["--api-key", "private"]]]),
        ),
        (
            "/invocations/globals/segments",
            json!([[["--api-key"]], [["private"]]]),
        ),
        ("/invocations/globals/segments", json!([[["--api-key"]]])),
        (
            "/command_sets/inspection",
            json!({"--api-key=private":"base-private"}),
        ),
        ("/command_sets/inspection/status", json!("bad/id")),
        ("/command_sets/inspection/status", json!("publish")),
        ("/rules/0/conditions/executable", json!({"glob":"acme*"})),
    ] {
        let mut invalid = original.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        changes.push(invalid);
    }
    let mut collision = original.clone();
    collision["rules"][6]["id"] = json!("w0-base-status");
    changes.push(collision);
    let mut injected = original.clone();
    injected["invocations"]["globals"]["extent"] = json!([1, 0]);
    changes.push(injected);
    let mut missing = original.clone();
    missing["capabilities"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v != "command-sets-v1");
    changes.push(missing);
    let mut reused = original.clone();
    reused["rules"][1]["command_set"] = json!("inspection");
    changes.push(reused);
    for invalid in changes {
        assert!(Pack::from_yaml(&invalid.to_string()).is_err(), "{invalid}");
    }
}

#[test]
fn precondition_identities_share_the_member_and_invocation_namespace() {
    let with_precondition = |id: &str| {
        let mut source = document();
        source["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!("preconditions-v1"));
        source["preconditions"] = json!([{"id":id,"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requires_any":[{"executable":"acme"}],"on_unmet":{"decision":"deny","message":"Use Acme."}}]);
        source.to_string()
    };
    assert!(Pack::from_yaml(&with_precondition("acme-tool")).is_ok());
    for id in ["base-status", "w0-publish", "w3-base-diff"] {
        assert!(Pack::from_yaml(&with_precondition(id)).is_err(), "{id}");
    }
}

#[test]
fn five_million_virtual_rules_compile_as_five_thousand_bodies() {
    let mut source = document();
    source["invocations"] = json!({"globals":{"id_prefix":"v","segments":(0..3).map(|phase|(0..10).map(|n|vec![format!("phase{phase}-{n}")]).collect::<Vec<_>>()).collect::<Vec<_>>()}});
    source.as_object_mut().unwrap().remove("command_sets");
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "invocation-groups-v1"
    ]);
    source["rules"] = json!((0..5000).map(|n|json!({"id":format!("r{n}"),"invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[format!("op{n}")]}},"decision":"allow"})).collect::<Vec<_>>());
    let plan = Plan::compile(
        vec![(
            Pack::from_yaml(&source.to_string()).unwrap(),
            Profile::Recommended,
        )],
        None,
        json!({}),
    )
    .unwrap();
    // One posting per body; dispatch bindings are read from the body keys.
    assert_eq!(
        u32::from_le_bytes(plan.bytes()[12..16].try_into().unwrap()),
        5000
    );
    assert!(plan.bytes().len() < 4 * 1024 * 1024);
    let result = plan
        .evaluate(
            &request(&["phase0-9", "phase1-1", "phase2-2", "op4999"]),
            &|| Ok(()),
            true,
            true,
        )
        .unwrap();
    assert_eq!(result.decision, policy::Decision::Allow);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].rule_id, "v912-r4999");
    assert!(
        policy::inspect_pack_rule(&source.to_string(), "v912-r4999")
            .unwrap()
            .is_some()
    );
}

#[test]
fn many_bound_executables_keep_metadata_constant_and_dispatch_only_their_own() {
    let mut source = document();
    source["invocations"] = json!({"globals":{"id_prefix":"v","segments":[[[],["--plain"]]]}});
    source.as_object_mut().unwrap().remove("command_sets");
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "invocation-groups-v1"
    ]);
    let compile = |count: usize| {
        let mut source = source.clone();
        source["rules"] = json!((0..count).map(|n|json!({"id":format!("r{n}"),"invocation":"globals","conditions":{"domain":"argv","executable":format!("acme-{n}"),"arguments":{"kind":"exact","values":["inspect"]}},"decision":"allow"})).collect::<Vec<_>>());
        Plan::compile(
            vec![(
                Pack::from_yaml(&source.to_string()).unwrap(),
                Profile::Recommended,
            )],
            None,
            json!({}),
        )
        .unwrap()
    };
    let metadata = |plan: &Plan| u32::from_le_bytes(plan.bytes()[8..12].try_into().unwrap());
    let postings = |plan: &Plan| u32::from_le_bytes(plan.bytes()[12..16].try_into().unwrap());
    let (small, large) = (compile(10), compile(2000));
    // Bindings live in body keys: neither metadata nor postings grow per binding.
    assert_eq!(metadata(&small), metadata(&large));
    assert_eq!((postings(&small), postings(&large)), (10, 2000));
    let restored = Plan::load(large.bytes().to_vec()).unwrap();
    let mut call = request(&["--plain", "inspect"]);
    call.executable = "acme-1999".into();
    let result = restored.evaluate(&call, &|| Ok(()), true, true).unwrap();
    assert_eq!(result.decision, policy::Decision::Allow);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].rule_id, "v1-r1999");
    call.executable = "acme-2000".into();
    assert!(
        restored
            .evaluate(&call, &|| Ok(()), true, true)
            .unwrap()
            .matches
            .is_empty()
    );
    // A body key leads with executable, owner, group and set; corrupting
    // its group or retargeting its executable refuses on full validation.
    let binding = b"\x03acme-3\x000\0globals\0\0inspect\0";
    let mut bytes = small.bytes().to_vec();
    let at = bytes
        .windows(binding.len())
        .position(|w| w == binding)
        .unwrap();
    bytes[at + 10] = b'G';
    assert!(Plan::load(bytes.clone()).is_err());
    bytes[at + 10] = b'g';
    bytes[at + 1] = b'b';
    assert!(Plan::load(bytes.clone()).is_err());
    // A key whose owner field disagrees with its entry refuses at dispatch.
    bytes[at + 1] = b'a';
    bytes[at + 8] = b'1';
    let mut call = request(&["--plain", "inspect"]);
    call.executable = "acme-3".into();
    assert!(Plan::load(bytes.clone()).is_err());
    assert!(
        Plan::load_authenticated(bytes)
            .unwrap()
            .evaluate(&call, &|| Ok(()), true, true)
            .is_err()
    );
}

#[test]
fn shared_bindings_keep_pack_ownership_fallback_and_custom_authority() {
    let mut first = document();
    first["rules"][0]["fallback"] = json!(true);
    first["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("fallback-rules-v1"));
    let mut second = document();
    second["id"] = json!("acme/other");
    second["rules"][0]["decision"] = json!("ask");
    let custom_source = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"override","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["status"]}},"decision":"allow"}]}).to_string();
    for with_custom in [false, true] {
        let custom = with_custom.then(|| policy::CustomRules::from_yaml(&custom_source).unwrap());
        let left = Pack::from_yaml(&first.to_string()).unwrap();
        let right = Pack::from_yaml(&second.to_string()).unwrap();
        let plan = Plan::compile(
            vec![
                (
                    Pack::from_yaml(&first.to_string()).unwrap(),
                    Profile::Recommended,
                ),
                (
                    Pack::from_yaml(&second.to_string()).unwrap(),
                    Profile::Recommended,
                ),
            ],
            with_custom.then(|| policy::CustomRules::from_yaml(&custom_source).unwrap()),
            json!({}),
        )
        .unwrap();
        for args in [vec!["status"], vec!["--plain", "status"]] {
            let request = request(&args);
            let expected = evidence(
                policy::evaluate_selected(
                    &[
                        (&left, Profile::Recommended),
                        (&right, Profile::Recommended),
                    ],
                    &custom.as_ref().into_iter().collect::<Vec<_>>(),
                    &request,
                )
                .unwrap(),
            );
            let actual = evidence(plan.evaluate(&request, &|| Ok(()), true, true).unwrap());
            assert_eq!(actual, expected);
            assert_eq!(
                actual["decision"],
                if with_custom && args.len() == 1 {
                    "allow"
                } else {
                    "ask"
                }
            );
            assert_eq!(
                actual["matches"].as_array().unwrap().len(),
                if with_custom && args.len() == 1 { 3 } else { 2 }
            );
        }
    }
}

#[test]
fn ambiguous_large_bodies_fail_before_virtual_candidate_amplification() {
    let mut source = document();
    source["invocations"] = json!({"globals":{"id_prefix":"v","segments":vec![vec![Vec::<String>::new(),vec!["x".into()]];12]}});
    source.as_object_mut().unwrap().remove("command_sets");
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "invocation-groups-v1"
    ]);
    source["rules"] = json!([{"id":"wide","invocation":"globals","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":vec!["z".repeat(4096);244]}},"decision":"allow"}]);
    let pack = Pack::from_yaml(&source.to_string()).unwrap();
    let request = request(&["x"; 12]);
    assert!(matches!(
        policy::evaluate_selected(&[(&pack, Profile::Recommended)], &[], &request),
        Err(Error::Limit)
    ));
    let plan = Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    assert!(matches!(
        plan.evaluate(&request, &|| Ok(()), true, true),
        Err(Error::Limit)
    ));
}
