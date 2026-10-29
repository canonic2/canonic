//! Full explanation parity and checked-record rejection, including broad candidates.
use super::*;
use crate::policy::{self, Decision};
use serde_json::{Value, json};

fn sources() -> (String, String) {
    let rule = |id, arguments, decision| json!({"id":id,"reason":"Acme inspection","conditions":{"domain":"argv","executable":"acme","arguments":arguments},"decision":{"loose":decision,"recommended":decision,"strict":decision}});
    let mut rows = vec![
        rule("broad", json!({"kind":"any"}), "ask"),
        rule(
            "ancestor",
            json!({"kind":"prefix","values":["read"]}),
            "deny",
        ),
        rule(
            "exact",
            json!({"kind":"exact","values":["read",""]}),
            "allow",
        ),
        rule(
            "options",
            json!({"kind":"options","prefix":["read","safe"],"flags":["-n"],"values":["--format"],"operands":false}),
            "allow",
        ),
        rule(
            "risk",
            json!({"kind":"risk_tokens","prefix":["read"],"any_of":["--force",{"prefix":"/outside"}]}),
            "deny",
        ),
    ];
    rows[2]["conditions"]["arguments"] = json!({"kind":"exact","values":["read", "context-bound"]});
    let pack = json!({"schema_version":1,"id":"acme/a","version":"0.1.0","description":"Acme","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],"rules":rows,"kind":"pack"}).to_string();
    let custom = json!({"schema_version":1,"rules":[{"id":"custom","decision":"allow","reason":"Acme custom","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read","custom"]}}},{"id":"custom-risk","decision":"deny","reason":"Acme custom risk","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["read","custom","risk"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    (pack, custom)
}

#[test]
fn compiled_metadata_requires_current_policy_schema_and_document_kind() {
    let (source, custom_source) = sources();
    let plan = Plan::compile(
        vec![(Pack::from_yaml(&source).unwrap(), Profile::Recommended)],
        Some(CustomRules::from_yaml(&custom_source).unwrap()),
        json!({}),
    )
    .unwrap();
    let length = read_number(plan.bytes(), 8).unwrap();
    let original: Value = serde_json::from_slice(&plan.bytes()[HEADER..HEADER + length]).unwrap();
    for pointer in ["/packs/0/document", "/custom/document"] {
        let kind = if pointer.starts_with("/packs") {
            "junk"
        } else {
            "broken"
        };
        for (field, invalid) in [("schema_version", json!(2)), ("kind", json!(kind))] {
            let mut metadata = original.clone();
            metadata.pointer_mut(pointer).unwrap()[field] = invalid;
            let encoded = serde_json::to_vec(&metadata).unwrap();
            // Keep offsets intact while replacing only the metadata descriptor.
            assert_eq!(encoded.len(), length);
            let mut bytes = plan.bytes().to_vec();
            bytes[HEADER..HEADER + length].copy_from_slice(&encoded);
            assert!(Plan::load(bytes.clone()).is_err(), "{pointer} {field}");
            assert!(
                Plan::load_authenticated(bytes).is_err(),
                "{pointer} {field}"
            );
        }
    }
}

#[test]
fn persisted_postings_preserve_complete_reference_evidence_and_unknown_context() {
    let (source, custom_source) = sources();
    let reference = Pack::from_yaml(&source).unwrap();
    let custom = CustomRules::from_yaml(&custom_source).unwrap();
    let plan = Plan::compile(
        vec![(Pack::from_yaml(&source).unwrap(), Profile::Recommended)],
        Some(CustomRules::from_yaml(&custom_source).unwrap()),
        json!({}),
    )
    .unwrap();
    let loaded = Plan::load(plan.bytes().to_vec()).unwrap();
    let authenticated = Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
    for cwd in ["/acme", "/acme/other"] {
        for args in [
            vec![],
            vec!["read"],
            vec!["read", ""],
            vec!["read", "safe", "-n"],
            vec!["read", "safe", "--format", "--force"],
            vec!["read", "--", "--force"],
            vec!["read", "custom"],
            vec!["read", "custom", "risk"],
            vec!["read", "/outside"],
            vec!["unknown"],
        ] {
            for (known_cwd, known_arguments) in
                [(true, true), (false, true), (true, false), (false, false)]
            {
                let request = Request {
                    executable: "acme".into(),
                    arguments: args.iter().map(|s| (*s).into()).collect(),
                    cwd: cwd.into(),
                    project: "/acme".into(),
                };
                let expected = policy::evaluate_evidence(
                    &[(&reference, Profile::Recommended)],
                    &[&custom],
                    &request,
                    false,
                    &|| Ok(()),
                    policy::Knowledge {
                        cwd: known_cwd,
                        arguments: known_arguments,
                    },
                    None,
                )
                .unwrap();
                let actual = loaded
                    .evaluate(&request, &|| Ok(()), known_cwd, known_arguments)
                    .unwrap();
                assert_eq!(
                    json!(actual),
                    json!(expected),
                    "{args:?} {cwd} {known_cwd}/{known_arguments}"
                );
                assert_eq!(
                    json!(
                        authenticated
                            .evaluate(&request, &|| Ok(()), known_cwd, known_arguments)
                            .unwrap()
                    ),
                    json!(expected)
                );
            }
        }
    }
    let value: Value = json!(
        loaded
            .evaluate(
                &Request {
                    executable: "acme".into(),
                    arguments: vec!["read".into(), "custom".into()],
                    cwd: "/acme".into(),
                    project: "/acme".into()
                },
                &|| Ok(()),
                true,
                true
            )
            .unwrap()
    );
    assert_eq!(value["decision"], json!(Decision::Allow));
    assert_eq!(value["matches"].as_array().unwrap().len(), 3);
}

#[test]
fn environment_field_patterns_decide_alike_through_plans_and_sources() {
    // Variables named like selector keys stay data; their selectors are patterns.
    let fields =
        json!({"regex":{"glob":"acme-*"},"exact":{"regex":"^v[0-9]$"},"ACME_MODE":"literal"});
    let source = json!({"schema_version":1,"id":"acme/env","version":"0.1.0","description":"Acme","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1","glob-v1","regex-v1"],"kind":"pack",
        "rules":[{"id":"env","decision":"allow","reason":"Acme environment","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]},"environment":{"fields":fields}}}]}).to_string();
    let reference = Pack::from_yaml(&source).unwrap();
    let plan = Plan::load(
        Plan::compile(
            vec![(Pack::from_yaml(&source).unwrap(), Profile::Recommended)],
            None,
            json!({}),
        )
        .unwrap()
        .bytes()
        .to_vec(),
    )
    .unwrap();
    let request = Request {
        executable: "acme".into(),
        arguments: vec!["read".into()],
        cwd: "/acme".into(),
        project: "/acme".into(),
    };
    let input = normalized::Input::Argv {
        executable: request.executable.clone(),
        arguments: request.arguments.clone(),
        original_input: json!({}),
    };
    let now = crate::evidence::now();
    let mut decisions = Vec::new();
    for environment in [
        Some([
            ("regex", "acme-1"),
            ("exact", "v1"),
            ("ACME_MODE", "literal"),
        ]),
        Some([
            ("regex", "acme-1"),
            ("exact", "v10"),
            ("ACME_MODE", "literal"),
        ]),
        Some([
            ("regex", "other"),
            ("exact", "v1"),
            ("ACME_MODE", "literal"),
        ]),
        Some([
            ("regex", "acme-*"),
            ("exact", "^v[0-9]$"),
            ("ACME_MODE", "lit"),
        ]),
        None,
    ] {
        let context = normalized::Context {
            working_files: None,
            cwd: request.cwd.clone(),
            environment: environment.map(|pairs| {
                pairs
                    .iter()
                    .map(|(name, value)| ((*name).into(), (*value).into()))
                    .collect()
            }),
        };
        let facts = Facts {
            input: &input,
            context: &context,
            executable: None,
            now: &now,
            known_cwd: true,
            known_arguments: true,
            locations: Some(&policy::resources::Unavailable),
            effects: None,
        };
        let expected = policy::evaluate_facts(
            &[(&reference, Profile::Recommended)],
            &[],
            &request,
            &facts,
            false,
            &|| Ok(()),
            None,
            None,
        )
        .unwrap();
        let actual = plan.evaluate_input(&request, &facts, &|| Ok(())).unwrap();
        assert_eq!(json!(actual), json!(expected), "{environment:?}");
        decisions.push(expected.decision);
    }
    assert_eq!(
        decisions,
        [
            Decision::Allow,
            Decision::Ask,
            Decision::Ask,
            Decision::Ask,
            Decision::Ask
        ]
    );
}

#[test]
fn truncated_overflowed_reordered_and_trailing_images_are_refused() {
    let (source, _) = sources();
    let plan = Plan::compile(
        vec![(Pack::from_yaml(&source).unwrap(), Profile::Loose)],
        None,
        json!({}),
    )
    .unwrap();
    let original = plan.bytes();
    for length in [0, 7, 15, original.len() - 1] {
        assert!(Plan::load(original[..length].to_vec()).is_err());
    }
    for at in [0, 8, 12, HEADER + read_number(original, 8).unwrap()] {
        let mut bytes = original.to_vec();
        bytes[at..at + 4].fill(255);
        assert!(Plan::load(bytes).is_err());
    }
    let mut bytes = original.to_vec();
    bytes.push(0);
    assert!(Plan::load(bytes).is_err());
    let table = HEADER + read_number(original, 8).unwrap();
    let mut bytes = original.to_vec();
    let first = bytes[table..table + ENTRY].to_vec();
    let second = bytes[table + ENTRY..table + 2 * ENTRY].to_vec();
    bytes[table..table + ENTRY].copy_from_slice(&second);
    bytes[table + ENTRY..table + 2 * ENTRY].copy_from_slice(&first);
    assert!(Plan::load(bytes).is_err());
}

#[test]
fn valid_wide_unreachable_argv_rules_are_retained_without_changing_source_admission() {
    let (source, _) = sources();
    let mut value: Value = serde_json::from_str(&source).unwrap();
    value["rules"][0]["conditions"]["arguments"] =
        json!({"kind":"exact","values":vec!["a".repeat(4096);32]});
    let pack = Pack::from_yaml(&value.to_string()).unwrap();
    let plan = Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    assert!(Plan::load(plan.bytes().to_vec()).is_ok());
}
