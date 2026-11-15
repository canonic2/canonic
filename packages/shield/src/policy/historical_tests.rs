//! Retained matching decisions survive the authenticated compiled boundary.
use super::*;
use resolution::compiled;
use serde_json::{Value, json};

fn old_pack(rows: &[Value]) -> String {
    json!({"schema_version":2,"id":"acme/legacy","version":"0.1.0",
        "description":"Acme policy","license":"MIT","maintainers":["Acme"],
        "engine":"^0.1","capabilities":["argv-v1","possible-effects-v1"],
        "rules":rows})
    .to_string()
}

fn old_row(executable: &str, args: &[String], decision: &str) -> Value {
    json!({"id":"read","matcher":{"executable":executable,
        "arguments":{"kind":"exact","values":args}},"reason":"Acme policy",
        "profiles":{"loose":decision,"recommended":decision,"strict":decision}})
}

fn read_request() -> Request {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Request {
        executable: "shield:read-text".into(),
        arguments: vec![format!("{cwd}/acme.txt")],
        cwd: cwd.clone(),
        project: cwd,
    }
}

fn read_input(request: &Request) -> normalized::Input {
    normalized::Input::File {
        action: "read".into(),
        sources: vec![request.arguments[0].clone()],
        destination: None,
        recursive: false,
        original_input: json!({"file_path":request.arguments[0]}),
    }
}

fn exact_customs(conditions: &Value, decision: &str) -> [(&'static str, CustomRules); 2] {
    let historical = json!({"schema_version":1,"rules":[{
        "schema_version":1,"id":"exact","decision":decision,
        "conditions":conditions,"reason":null,"expires_at":null
    }]})
    .to_string();
    let current = rules::document(&[json!({"id":"exact","decision":decision,
        "conditions":conditions})])
    .unwrap()
    .to_string();
    [
        (
            "historical",
            CustomRules::from_retained_yaml(&historical).unwrap(),
        ),
        ("current", CustomRules::from_yaml(&current).unwrap()),
    ]
}

fn broad_pack(decision: &str) -> Pack {
    Pack::from_yaml(&json!({"schema_version":1,"kind":"pack","id":"acme/broad",
        "version":"0.1.0","description":"Acme policy","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"broad","decision":{"loose":decision,"recommended":decision,"strict":decision},
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]
    }).to_string()).unwrap()
}

fn working_file_variants(cwd: &str) -> [Option<crate::location::WorkingFiles>; 3] {
    [
        None,
        Some(crate::location::WorkingFiles::default()),
        Some(crate::location::WorkingFiles {
            read: vec![std::path::PathBuf::from(cwd).join("reference")],
            write: vec![std::path::PathBuf::from(cwd).join("scratch")],
            ..Default::default()
        }),
    ]
}

fn check_read(
    packs: Vec<Pack>,
    custom: Option<CustomRules>,
    expected: Decision,
    request: &Request,
) {
    let input = read_input(request);
    let context = normalized::Context {
        cwd: request.cwd.clone(),
        environment: None,
        working_files: None,
    };
    let facts = rules::Facts {
        input: &input,
        context: &context,
        executable: None,
        now: "2026-01-01T00:00:00Z",
        known_cwd: true,
        known_arguments: true,
        locations: Some(&resources::Unavailable),
        effects: None,
    };
    let selections: Vec<_> = packs
        .iter()
        .map(|pack| (pack, Profile::Recommended))
        .collect();
    let customs: Vec<_> = custom.iter().collect();
    let reference = evaluate_facts(
        &selections,
        &customs,
        request,
        &facts,
        false,
        &|| Ok(()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(reference.decision, expected);
    let plan = compiled::Plan::compile(
        packs
            .into_iter()
            .map(|pack| (pack, Profile::Recommended))
            .collect(),
        custom,
        json!({}),
    )
    .unwrap();
    for loaded in [
        compiled::Plan::load(plan.bytes().to_vec()).unwrap(),
        compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap(),
    ] {
        let actual = loaded.evaluate_input(request, &facts, &|| Ok(())).unwrap();
        assert_eq!(actual.decision, expected);
        assert_eq!(json!(actual.matches), json!(reference.matches));
    }
}

#[test]
fn historical_compiled_sdk_read_keeps_argv_decisions_and_exact_scope() {
    let request = read_request();
    for (authored, expected) in [("allow", Decision::Allow), ("deny", Decision::Deny)] {
        let source = old_pack(&[old_row(&request.executable, &request.arguments, authored)]);
        check_read(
            vec![Pack::from_retained_yaml(&source).unwrap()],
            None,
            expected,
            &request,
        );
        let mut other = request.clone();
        other.arguments[0].push_str(".other");
        check_read(
            vec![Pack::from_retained_yaml(&source).unwrap()],
            None,
            Decision::Ask,
            &other,
        );
    }
}

#[test]
fn historical_compiled_sdk_allow_cannot_override_current_file_deny() {
    let request = read_request();
    let source = json!({"schema_version":1,"rules":[{
        "id":"read","scope":"project","decision":"allow","reason":"Acme policy",
        "matcher":{"executable":request.executable,
            "arguments":{"kind":"exact","values":request.arguments}}
    }]})
    .to_string();
    let current = json!({"schema_version":1,"kind":"pack","id":"acme/current",
        "version":"0.1.0","description":"Acme policy","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":"*"},"capabilities":["structured-rules-v1","location-v1"],
        "rules":[{"id":"deny-read","decision":{"loose":"deny","recommended":"deny","strict":"deny"},
            "conditions":{"domain":"file","action":"read"}}]})
    .to_string();
    check_read(
        vec![Pack::from_yaml(&current).unwrap()],
        Some(CustomRules::from_retained_yaml(&source).unwrap()),
        Decision::Deny,
        &request,
    );
}

#[test]
fn historical_compiled_unmatched_effect_rule_still_requires_review() {
    let request = read_request();
    let mut effect = old_row("acme", &[], "deny");
    effect["id"] = json!("network");
    effect["matcher"]["possible_effects"] = json!(["network"]);
    let source = old_pack(&[
        old_row(&request.executable, &request.arguments, "allow"),
        effect,
    ]);
    check_read(
        vec![Pack::from_retained_yaml(&source).unwrap()],
        None,
        Decision::Ask,
        &request,
    );
}

#[test]
fn historical_compiled_empty_pack_retains_its_original_header_limits() {
    let mut source: Value = serde_json::from_str(&old_pack(&[])).unwrap();
    source["maintainers"] = json!(["M".repeat(4096), "M".repeat(4096)]);
    source["version"] = json!(format!("0.1.0+{}", "a".repeat(200)));
    let pack = Pack::from_retained_yaml(&source.to_string()).unwrap();
    let plan =
        compiled::Plan::compile(vec![(pack, Profile::Recommended)], None, json!({})).unwrap();
    assert!(compiled::Plan::load(plan.bytes().to_vec()).is_ok());
    assert!(compiled::Plan::load_authenticated(plan.bytes().to_vec()).is_ok());
}

#[test]
fn historical_partial_facts_preserve_original_abstention_for_all_decisions() {
    // The historical matcher and normalized evaluators required both facts
    // for every decision.
    let request = Request {
        executable: "rm".into(),
        arguments: vec!["$X".into()],
        ..read_request()
    };
    let input = normalized::Input::Argv {
        executable: request.executable.clone(),
        arguments: request.arguments.clone(),
        original_input: json!({}),
    };
    let context = normalized::Context {
        cwd: request.cwd.clone(),
        environment: None,
        working_files: None,
    };
    for (authored, decision) in [
        ("allow", Decision::Allow),
        ("ask", Decision::Ask),
        ("deny", Decision::Deny),
    ] {
        let mut matcher = old_row("rm", &[], authored);
        matcher["matcher"]["arguments"] = json!({"kind":"any"});
        let pack_source = old_pack(&[matcher.clone()]);
        matcher.as_object_mut().unwrap().remove("profiles");
        matcher["scope"] = json!("project");
        matcher["decision"] = json!(authored);
        let custom_source = json!({"schema_version":1,"rules":[matcher]}).to_string();
        let normalized_source = json!({"schema_version":1,"rules":[{
            "schema_version":1,"id":"remove","decision":authored,
            "conditions":{"domain":"argv","executable":"rm","arguments":{"kind":"any"}},
            "reason":null,"expires_at":null
        }]})
        .to_string();
        for (kind, source) in [
            ("pack", &pack_source),
            ("matcher", &custom_source),
            ("normalized", &normalized_source),
        ] {
            for (known_cwd, known_arguments) in
                [(true, true), (true, false), (false, true), (false, false)]
            {
                let facts = rules::Facts {
                    input: &input,
                    context: &context,
                    executable: None,
                    now: "2026-01-01T00:00:00Z",
                    known_cwd,
                    known_arguments,
                    locations: Some(&resources::Unavailable),
                    effects: None,
                };
                let packs = if kind == "pack" {
                    vec![Pack::from_retained_yaml(source).unwrap()]
                } else {
                    vec![]
                };
                let custom =
                    (kind != "pack").then(|| CustomRules::from_retained_yaml(source).unwrap());
                let selections: Vec<_> = packs.iter().map(|p| (p, Profile::Recommended)).collect();
                let customs: Vec<_> = custom.iter().collect();
                let reference = evaluate_facts(
                    &selections,
                    &customs,
                    &request,
                    &facts,
                    false,
                    &|| Ok(()),
                    None,
                    None,
                )
                .unwrap();
                let complete = known_cwd && known_arguments;
                assert_eq!(
                    reference.decision,
                    if complete { decision } else { Decision::Ask },
                    "{kind} {authored} cwd={known_cwd} arguments={known_arguments}"
                );
                assert_eq!(reference.matches.is_empty(), !complete);
                let plan = compiled::Plan::compile(
                    packs
                        .into_iter()
                        .map(|p| (p, Profile::Recommended))
                        .collect(),
                    custom,
                    json!({}),
                )
                .unwrap();
                for loaded in [
                    compiled::Plan::load(plan.bytes().to_vec()).unwrap(),
                    compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap(),
                ] {
                    let actual = loaded.evaluate_input(&request, &facts, &|| Ok(())).unwrap();
                    assert_eq!(actual.decision, reference.decision);
                    assert_eq!(json!(actual.matches), json!(reference.matches));
                }
            }
        }
    }
}

#[test]
fn historical_exact_mcp_requests_ignore_resolved_connection_only() {
    let request = read_request();
    let input = normalized::Input::Mcp {
        server: "acme".into(),
        tool: "read".into(),
        arguments: json!({"path":"reference"}),
        connection: None,
        original_input: json!({"name":"mcp__acme__read","arguments":{"path":"reference"}}),
    };
    let context = normalized::Context {
        cwd: request.cwd.clone(),
        environment: Some(BTreeMap::default()),
        working_files: Some(crate::location::WorkingFiles::default()),
    };
    let conditions = json!({"domain":"exact_request","input":input,
        "context":context,"executable":null});
    let mut resolved = input.clone();
    if let normalized::Input::Mcp { connection, .. } = &mut resolved {
        *connection = Some(json!({"transport":"stdio","command":"acme-mcp","args":[]}));
    }
    let mut variants = vec![(input.clone(), true), (resolved.clone(), true)];
    for field in ["server", "tool", "arguments", "original_input"] {
        let mut changed = resolved.clone();
        if let normalized::Input::Mcp {
            server,
            tool,
            arguments,
            original_input,
            ..
        } = &mut changed
        {
            match field {
                "server" => *server = "other".into(),
                "tool" => *tool = "other".into(),
                "arguments" => *arguments = json!({"path":"other"}),
                "original_input" => *original_input = json!({"other":true}),
                _ => unreachable!(),
            }
        }
        variants.push((changed, false));
    }
    for (authored, expected) in [
        ("allow", Decision::Allow),
        ("ask", Decision::Ask),
        ("deny", Decision::Deny),
    ] {
        for (family, custom) in exact_customs(&conditions, authored) {
            let plan = compiled::Plan::compile(vec![], Some(custom.clone()), json!({})).unwrap();
            let loaded = compiled::Plan::load(plan.bytes().to_vec()).unwrap();
            let authenticated = compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
            for (input, matches) in &variants {
                let facts = rules::Facts {
                    input,
                    context: &context,
                    executable: None,
                    now: "2026-01-01T00:00:00Z",
                    known_cwd: true,
                    known_arguments: true,
                    locations: Some(&resources::Unavailable),
                    effects: None,
                };
                let reference = evaluate_facts(
                    &[],
                    &[&custom],
                    &request,
                    &facts,
                    false,
                    &|| Ok(()),
                    None,
                    None,
                )
                .unwrap();
                let expected = if *matches { expected } else { Decision::Ask };
                assert_eq!(
                    reference.decision,
                    expected,
                    "{family} {authored} input={}",
                    json!(input)
                );
                assert_eq!(
                    reference
                        .matches
                        .iter()
                        .any(|m| m.rule_id == "exact" && m.applied),
                    *matches,
                    "{family} {authored} input={}",
                    json!(input)
                );
                for compiled in [&plan, &loaded, &authenticated] {
                    let actual = compiled
                        .evaluate_input(&request, &facts, &|| Ok(()))
                        .unwrap();
                    assert_eq!(actual.decision, reference.decision);
                    assert_eq!(json!(actual.matches), json!(reference.matches));
                }
            }
        }
    }
}

#[test]
fn historical_exact_requests_keep_current_and_compiled_parity_with_working_files() {
    let mut effect = old_row("unrelated", &[], "deny");
    effect["matcher"]["possible_effects"] = json!(["network"]);
    let effect_pack = Pack::from_retained_yaml(&old_pack(&[effect])).unwrap();
    let request = Request {
        executable: "acme".into(),
        arguments: vec!["read".into()],
        ..read_request()
    };
    let input = normalized::Input::Argv {
        executable: request.executable.clone(),
        arguments: request.arguments.clone(),
        original_input: json!({"command":"acme read"}),
    };
    let context = normalized::Context {
        cwd: request.cwd.clone(),
        environment: Some(BTreeMap::default()),
        working_files: None,
    };
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let executable = executable.to_str().unwrap();
    let conditions = json!({"domain":"exact_request","input":input,
        "context":context,"executable":executable});
    for (authored, expected) in [
        ("allow", Decision::Allow),
        ("ask", Decision::Ask),
        ("deny", Decision::Deny),
    ] {
        let opposite = if authored == "allow" { "deny" } else { "allow" };
        let pack = broad_pack(opposite);
        for (family, custom) in exact_customs(&conditions, authored) {
            let plan = compiled::Plan::compile(
                vec![
                    (pack.clone(), Profile::Recommended),
                    (effect_pack.clone(), Profile::Recommended),
                ],
                Some(custom.clone()),
                json!({}),
            )
            .unwrap();
            let loaded = compiled::Plan::load(plan.bytes().to_vec()).unwrap();
            let authenticated = compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
            // Historical specificity stays frozen; full evidence must agree
            // within each family while both families preserve the decision.
            let mut reference_matches = None;
            for working_files in working_file_variants(&request.cwd) {
                let context = normalized::Context {
                    working_files,
                    ..context.clone()
                };
                let facts = rules::Facts {
                    input: &input,
                    context: &context,
                    executable: Some(executable),
                    now: "2026-01-01T00:00:00Z",
                    known_cwd: true,
                    known_arguments: true,
                    locations: Some(&resources::Unavailable),
                    effects: None,
                };
                let reference = evaluate_facts(
                    &[
                        (&pack, Profile::Recommended),
                        (&effect_pack, Profile::Recommended),
                    ],
                    &[&custom],
                    &request,
                    &facts,
                    false,
                    &|| Ok(()),
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(
                    reference.decision, expected,
                    "{family} {authored} working_files={:?}",
                    context.working_files
                );
                assert!(
                    reference
                        .matches
                        .iter()
                        .any(|matched| matched.rule_id == "exact" && matched.applied)
                );
                let matches = json!(reference.matches);
                if let Some(baseline) = &reference_matches {
                    assert_eq!(&matches, baseline);
                } else {
                    reference_matches = Some(matches.clone());
                }
                for compiled in [&plan, &loaded, &authenticated] {
                    let actual = compiled
                        .evaluate_input(&request, &facts, &|| Ok(()))
                        .unwrap();
                    assert_eq!(actual.decision, expected, "compiled {family} {authored}");
                    assert_eq!(json!(actual.matches), matches);
                }
            }
        }
    }
}
