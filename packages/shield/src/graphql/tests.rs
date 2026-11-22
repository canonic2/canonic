use super::*;
use crate::policy::{self, CustomRules, Decision, Request};
use serde_json::json;
use std::fmt::Write as _;

fn model() -> Value {
    json!({"root":"query","states":{
        "query":{"types":["Query"],"fields":{"repository":{"arguments":{
            "owner":{"required":true,"match":{"text":"acme"}},
            "name":{"required":true,"match":{"text":"web"}}
        },"select":"repository"}}},
        "repository":{"types":["Repository"],"fields":{"nameWithOwner":{"leaf":true},"issues":{"arguments":{
            "first":{"match":{"scalar":"integer"}},"after":{"nullable":true,"cursor":true,"match":{"scalar":"string"}}
        },"select":"connection"}}},
        "connection":{"types":["IssueConnection"],"fields":{"totalCount":{"leaf":true}}}
    }})
}
fn custom() -> CustomRules {
    CustomRules::from_yaml(&json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","requirements-v1","graphql-v1"],
        "rules":[{"id":"read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},
            "requirements":{"graphql":{"carrier":"gh-api","operation":"query","selection":model()}},
            "decision":"allow","on_unmet":{"decision":"deny"}}]
    }).to_string()).unwrap()
}
fn request(query: &str) -> Request {
    Request {
        executable: "gh".into(),
        arguments: vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
        ],
        cwd: "/workspace".into(),
        project: "/workspace".into(),
    }
}

fn root_model() -> Value {
    json!({"root":"query","states":{
        "query":{"types":["Query"],"fields":{"repository":{"subtree":true,"arguments":{
            "owner":{"required":true,"match":{"text":"acme"}},
            "name":{"required":true,"match":{"text":"web"}}
        }}}}
    }})
}

const SUBTREE_CASES: &[(&str, &[&str], Decision)] = &[
    (
        r#"{repository(owner:"acme",name:"web"){pullRequest(number:12){reviewThreads(first:100){nodes{comments(first:100){nodes{outdated diffHunk commit{oid}}}}}}}}"#,
        &[],
        Decision::Allow,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){owner{repositories(first:10){nodes{name}}} parent{name} future(filter:{enabled:true}){newField}}}"#,
        &[],
        Decision::Allow,
    ),
    (
        r#"{...Roots} fragment Roots on Query{a:repository(owner:"acme",name:"web"){...Data} b:repository(owner:"acme",name:"web"){... on Repository{unknown}}} fragment Data on Repository{newField}"#,
        &[],
        Decision::Allow,
    ),
    (
        r#"query($owner:String!="acme",$endCursor:String){repository(owner:$owner,name:"web"){anything(after:$endCursor){nodes{newField}}}}"#,
        &["--paginate"],
        Decision::Allow,
    ),
    (
        r#"{repository(owner:"other",name:"web"){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"other"){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){name} ...Bad} fragment Bad on Query{repository(owner:"other",name:"web") @skip(if:true){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){...Shared} ...Shared} fragment Shared on Query{viewer{login}}"#,
        &[],
        Decision::Deny,
    ),
    (r"{repository:viewer{login}}", &[], Decision::Deny),
    (
        r#"{repository(owner:"acme",name:"web"){name} search(query:"all"){nodes{id}}}"#,
        &[],
        Decision::Deny,
    ),
    (r#"{repository(owner:"acme"){name}}"#, &[], Decision::Deny),
    (
        r#"{repository(owner:"acme",name:"web",extra:1){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"mutation{repository(owner:"acme",name:"web"){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"subscription{repository(owner:"acme",name:"web"){name}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){future @unknown}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){...Missing}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){...Cycle}} fragment Cycle on Repository{...Cycle}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"{repository(owner:"acme",name:"web"){future(filter:{x:1,x:2})}}"#,
        &[],
        Decision::Deny,
    ),
    (
        r#"query($endCursor:String){repository(owner:$endCursor,name:"web"){name}}"#,
        &["--paginate"],
        Decision::Deny,
    ),
];

#[test]
fn graphql_subtree_checks_every_root_without_a_descendant_inventory() {
    let source = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","requirements-v1","graphql-v1","graphql-subtree-v1"],
        "rules":[{"id":"read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},
            "requirements":{"graphql":{"carrier":"gh-api","operation":"query","selection":root_model()}},
            "decision":"allow","on_unmet":{"decision":"deny"}}]
    });
    let rules = CustomRules::from_yaml(&source.to_string()).unwrap();
    let plan = policy::resolution::compiled::Plan::compile(vec![], Some(rules.clone()), json!({}))
        .unwrap();
    let plan = policy::resolution::compiled::Plan::load(plan.bytes().to_vec()).unwrap();
    for &(query, extra, expected) in SUBTREE_CASES {
        let mut request = request(query);
        request
            .arguments
            .extend(extra.iter().copied().map(str::to_owned));
        let reference =
            policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &request).unwrap();
        assert_eq!(reference.decision, expected, "{query}");
        let compiled = plan.evaluate(&request, &|| Ok(()), true, true).unwrap();
        assert_eq!(json!(compiled), json!(reference), "{query}");
    }
    let mut undeclared = source;
    undeclared["capabilities"].as_array_mut().unwrap().pop();
    assert!(CustomRules::from_yaml(&undeclared.to_string()).is_err());
}

#[test]
fn graphql_subtree_admission_is_explicit_and_query_root_only() {
    let valid = json!({"carrier":"gh-api","operation":"query","selection":root_model()});
    assert!(validate(&valid).is_ok());
    for (key, value) in [
        ("leaf", json!(true)),
        ("select", json!("query")),
        ("subtree", json!(false)),
    ] {
        let mut invalid = valid.clone();
        invalid["selection"]["states"]["query"]["fields"]["repository"][key] = value;
        assert!(validate(&invalid).is_err());
    }
    let mut nested = json!({"carrier":"gh-api","operation":"query","selection":model()});
    nested["selection"]["states"]["repository"]["fields"]["new"] = json!({"subtree":true});
    assert!(validate(&nested).is_err());
    let mut mutation = json!({"carrier":"gh-api","operation":"mutation","root_fields":["repository"],"selection":root_model()});
    mutation["selection"]["states"]["query"]["types"] = json!(["Mutation"]);
    assert!(validate(&mutation).is_err());
}

#[test]
fn graphql_query_classification_accepts_structural_object_literals_without_asserting_scope() {
    let plan = policy::resolution::compiled::Plan::compile(vec![], None, json!({})).unwrap();
    let carrier = carrier::decode(
        &request(r#"{repository(owner:"acme",name:"web"){future(filter:{enabled:true}){name}}}"#)
            .arguments,
        None,
    )
    .unwrap();
    let mut visits = 0;
    let document = document::parse(&carrier, &mut visits, &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(
        document
            .check(&read(None), &plan, &mut visits, &|| Ok(()))
            .unwrap()
            .is_none()
    );
}
#[test]
fn graphql_pagination_rejects_cursor_references_inside_query_objects() {
    for selection in [None, Some(root_model())] {
        let mut predicate = json!({"carrier":"gh-api","operation":"query"});
        let mut capabilities = vec![
            "structured-rules-v1",
            "argument-rules-v1",
            "requirements-v1",
            "graphql-v1",
        ];
        if let Some(model) = selection {
            predicate["selection"] = model;
            capabilities.push("graphql-subtree-v1");
        }
        let rules = CustomRules::from_yaml(&json!({"schema_version":1,"kind":"custom","capabilities":capabilities,
            "rules":[{"id":"read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},
                "requirements":{"graphql":predicate},"decision":"allow","on_unmet":{"decision":"deny"}}]
        }).to_string()).unwrap();
        let plan =
            policy::resolution::compiled::Plan::compile(vec![], Some(rules.clone()), json!({}))
                .unwrap();
        let plan = policy::resolution::compiled::Plan::load(plan.bytes().to_vec()).unwrap();
        for (filter, paginate, expected) in [
            ("{createdBy:$endCursor}", true, Decision::Deny),
            ("{nested:{createdBy:$endCursor}}", true, Decision::Deny),
            ("{after:$endCursor}", true, Decision::Deny),
            ("{createdBy:$actor}", true, Decision::Allow),
            ("{createdBy:$endCursor}", false, Decision::Allow),
        ] {
            let query = format!(
                r#"query($endCursor:String,$actor:String="acme"){{repository(owner:"acme",name:"web"){{issues(filterBy:{filter},after:$endCursor){{nodes{{id}}}}}}}}"#
            );
            let mut request = request(&query);
            if paginate {
                request.arguments.push("--paginate".into());
            }
            let reference =
                policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &request)
                    .unwrap();
            assert_eq!(
                reference.decision, expected,
                "{query} paginate={paginate} predicate={predicate}"
            );
            let compiled = plan.evaluate(&request, &|| Ok(()), true, true).unwrap();
            assert_eq!(json!(compiled), json!(reference));
        }
    }
}

#[test]
fn graphql_policy_admits_reads_and_rejects_neighboring_scope_escapes() {
    let rules = custom();
    for (query, expected) in [
        (
            r#"{ repository(owner:"acme",name:"web") { nameWithOwner } }"#,
            Decision::Allow,
        ),
        (
            r#"{ repository(owner:"other",name:"web") { nameWithOwner } }"#,
            Decision::Deny,
        ),
        (
            r#"{ repository(owner:"acme",name:"web") { nameWithOwner parent { nameWithOwner } } }"#,
            Decision::Deny,
        ),
        (
            r#"{ repository(owner:"acme",name:"web") { nameWithOwner } viewer { login } }"#,
            Decision::Deny,
        ),
        (r"{ repository:viewer { login } }", Decision::Deny),
        (
            r#"{ repository(owner:"acme",name:"web") { ...R } } fragment R on Repository { nameWithOwner }"#,
            Decision::Allow,
        ),
        (r"{ ...A } fragment A on Query { ...A }", Decision::Deny),
        (
            r#"{ repository(owner:"acme",owner:"other",name:"web") { nameWithOwner } }"#,
            Decision::Deny,
        ),
        (r"mutation { field }", Decision::Deny),
        (r"{ field(input:{x:1,x:2}) }", Decision::Deny),
    ] {
        let result = policy::evaluate(
            &[],
            Some(&rules),
            policy::Profile::Recommended,
            &request(query),
        )
        .unwrap();
        assert_eq!(result.decision, expected, "{query}");
    }
}
#[test]
fn graphql_variables_operation_selection_and_cursor_are_verified() {
    let rules = custom();
    let mut r = request(
        r#"query Read($owner:String!="acme",$endCursor:String) { repository(owner:$owner,name:"web") { issues(first:10,after:$endCursor) { totalCount } } } mutation Write { field }"#,
    );
    r.arguments.extend([
        "-f".into(),
        "operationName=Read".into(),
        "--paginate".into(),
    ]);
    assert_eq!(
        policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &r)
            .unwrap()
            .decision,
        Decision::Allow
    );
    r.arguments[3] = r.arguments[3].replace("owner:$owner", "owner:$endCursor");
    assert_eq!(
        policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &r)
            .unwrap()
            .decision,
        Decision::Deny
    );
}

#[test]
fn graphql_cursor_does_not_admit_an_undeclared_argument() {
    let rules = custom();
    let mut r = request(
        r#"query($endCursor:String){repository(owner:"acme",name:"web"){nameWithOwner(after:$endCursor)}}"#,
    );
    r.arguments.push("--paginate".into());
    assert_eq!(
        policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &r)
            .unwrap()
            .decision,
        Decision::Deny
    );
}
#[test]
fn graphql_model_admission_rejects_unknown_fields_references_and_unreachable_states() {
    let valid = json!({"carrier":"gh-api","operation":"query","selection":model()});
    assert!(validate(&valid).is_ok());
    let mut missing = valid.clone();
    missing["selection"]["states"]["query"]["fields"]["repository"]["select"] = json!("missing");
    assert!(validate(&missing).is_err());
    let mut unknown = valid.clone();
    unknown["future"] = json!(true);
    assert!(validate(&unknown).is_err());
    let mut unused = valid;
    unused["selection"]["states"]["unused"] =
        json!({"types":["Query"],"fields":{"name":{"leaf":true}}});
    assert!(validate(&unused).is_err());
}

#[test]
fn graphql_semantic_rules_cannot_export_command_prefix_allowances() {
    for requirements in [false, true] {
        let mut rule = json!({"id":"read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},"decision":"allow","agents":{"codex":{"prefixes":[["gh","api"]]}}});
        let predicate = json!({"carrier":"gh-api","operation":"query"});
        if requirements {
            rule["requirements"] = json!({"graphql":predicate});
            rule["on_unmet"] = json!({"decision":"ask"});
        } else {
            rule["conditions"]["graphql"] = predicate;
        }
        let source = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1","graphql-v1","requirements-v1"],"rules":[rule]});
        assert!(CustomRules::from_yaml(&source.to_string()).is_err());
    }
}

#[test]
fn graphql_recognizable_commands_without_applicable_predicates_do_no_analysis() {
    let source = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1","graphql-v1"],"rules":[{"id":"narrow","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"exact","values":["api","graphql","-fquery={viewer{login}}"]},"graphql":{"carrier":"gh-api","operation":"query"}},"decision":"allow"}]});
    let rules = CustomRules::from_yaml(&source.to_string()).unwrap();
    let plan = policy::resolution::compiled::Plan::compile(vec![], Some(rules), json!({})).unwrap();
    take_counts();
    assert_eq!(
        plan.evaluate(&request("{repository{name}}"), &|| Ok(()), true, true)
            .unwrap()
            .decision,
        Decision::Ask
    );
    assert_eq!(take_counts(), [0; 4]);
}

fn pack() -> policy::Pack {
    policy::Pack::from_yaml(&json!({"schema_version":1,"kind":"pack","id":"acme/graphql","version":"0.1.0",
        "description":"Read fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},
        "capabilities":["structured-rules-v1","argument-rules-v1","graphql-v1","preconditions-v1"],
        "preconditions":[{"id":"scope","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},
            "requires_any":[{"graphql":{"carrier":"gh-api","operation":"query","selection":model()}}],"on_unmet":{"decision":"deny"}}],
        "rules":[{"id":"read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]},"graphql":{"carrier":"gh-api","operation":"query"}},"decision":"allow"},
            {"id":"same-read","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]},"graphql":{"carrier":"gh-api","operation":"query"}},"decision":"allow"}]
    }).to_string()).unwrap()
}

#[test]
fn graphql_compiled_and_reference_share_analysis_and_preserve_complete_decision_evidence() {
    let source = pack();
    let plan = policy::resolution::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    let loaded = policy::resolution::compiled::Plan::load(plan.bytes().to_vec()).unwrap();
    for query in [
        r#"{repository(owner:"acme",name:"web"){nameWithOwner}}"#,
        r#"{repository(owner:"other",name:"web"){nameWithOwner}}"#,
    ] {
        let r = request(query);
        take_counts();
        let expected = policy::evaluate(
            std::slice::from_ref(&source),
            None,
            policy::Profile::Recommended,
            &r,
        )
        .unwrap();
        let counts = take_counts();
        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 1);
        let actual = loaded.evaluate(&r, &|| Ok(()), true, true).unwrap();
        let counts = take_counts();
        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 1);
        assert_eq!(json!(actual), json!(expected));
    }
}

#[test]
fn graphql_unrelated_commands_do_not_decode_parse_or_load_models() {
    let plan = policy::resolution::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    let plan =
        policy::resolution::compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
    for (executable, args) in [
        ("ls", vec![]),
        ("gh", vec!["pr", "list"]),
        ("gh", vec!["--version"]),
    ] {
        let mut r = request("");
        r.executable = executable.into();
        r.arguments = args.into_iter().map(str::to_owned).collect();
        take_counts();
        plan.evaluate(&r, &|| Ok(()), true, true).unwrap();
        assert_eq!(take_counts(), [0; 4]);
    }
    let r = request(r#"{repository(owner:"acme",name:"web"){nameWithOwner}}"#);
    plan.evaluate(&r, &|| Ok(()), true, true).unwrap();
    assert_eq!(take_counts(), [1, 1, 2, 1]);
    plan.evaluate(&r, &|| Ok(()), true, true).unwrap();
    assert_eq!(take_counts(), [1, 1, 2, 0]);
}

#[test]
fn graphql_unsupported_results_are_reused_without_parsing_or_model_loading() {
    let plan = policy::resolution::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    let mut r = request("{viewer{login}}");
    r.arguments
        .extend(["--input".into(), "request.json".into()]);
    take_counts();
    assert_eq!(
        plan.evaluate(&r, &|| Ok(()), true, true).unwrap().decision,
        Decision::Deny
    );
    assert_eq!(take_counts(), [1, 0, 0, 0]);
}

#[test]
fn graphql_analysis_limits_block_instead_of_falling_back_and_cancellation_propagates() {
    let r = request(&format!("{}name{}", "{field ".repeat(80), "}".repeat(80)));
    assert!(policy::evaluate(&[], Some(&custom()), policy::Profile::Recommended, &r).is_err());
    let plan = policy::resolution::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    assert!(
        plan.evaluate(
            &request("{viewer{login}}"),
            &|| Err(policy::Error::Limit),
            true,
            true
        )
        .is_err()
    );
}
#[test]
fn graphql_carrier_keeps_raw_and_typed_fields_distinct() {
    for args in [
        vec!["api", "graphql", "-fquery={viewer{login}}"],
        vec![
            "api",
            "--raw-field=query={viewer{login}}",
            "graphql",
            "--paginate=false",
        ],
    ] {
        assert!(
            carrier::decode(
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                None
            )
            .is_ok()
        );
    }
    for args in [
        vec!["api", "graphql", "-Fquery=@input.graphql"],
        vec!["api", "graphql", "--input", "input.json"],
        vec![
            "api",
            "graphql",
            "-fquery={viewer{login}}",
            "-Fquery={viewer{login}}",
        ],
        vec![
            "api",
            "graphql",
            "-fquery={viewer{login}}",
            "--hostname",
            "example.com",
        ],
    ] {
        assert!(
            carrier::decode(
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                None
            )
            .is_err()
        );
    }
}

#[test]
fn graphql_fragment_expansion_depth_is_bounded_in_both_validation_and_walk() {
    let plan = policy::resolution::compiled::Plan::compile(vec![], None, json!({})).unwrap();
    let mut reached_walk = false;
    for (fragments, levels, reverse) in [
        (16, 8, false),
        (16, 8, true),
        (63, 48, false),
        (63, 48, true),
    ] {
        let name = |index| {
            format!(
                "F{:02}",
                if reverse {
                    fragments - 1 - index
                } else {
                    index
                }
            )
        };
        let mut query = format!("{{ ...{} }}", name(0));
        for index in 0..fragments {
            let next = if index == fragments - 1 {
                "leaf".into()
            } else {
                format!("...{}", name(index + 1))
            };
            write!(
                query,
                " fragment {} on Query {{ {}{}{} }}",
                name(index),
                "field { ".repeat(levels),
                next,
                " }".repeat(levels)
            )
            .unwrap();
        }
        let carrier = carrier::decode(&request(&query).arguments, None).unwrap();
        let mut visits = 0;
        match document::parse(&carrier, &mut visits, &|| Ok(())) {
            Err(policy::Error::Limit) => {}
            Ok(Ok(document)) => {
                reached_walk = true;
                assert!(matches!(
                    document.check(&read(None), &plan, &mut visits, &|| Ok(())),
                    Err(policy::Error::Limit)
                ));
            }
            _ => panic!("fragment chain must parse or hit the structural depth bound"),
        }
        assert!(
            visits > fragments,
            "the per-definition parser must admit this fixture before Shield bounds fragment expansion"
        );
    }
    assert!(
        reached_walk,
        "exercise depth after memoized fragment validation too"
    );
    let carrier = carrier::decode(
        &request("{...A} fragment A on Query {field {...B}} fragment B on Query {leaf}").arguments,
        None,
    )
    .unwrap();
    let mut visits = 0;
    let document = document::parse(&carrier, &mut visits, &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(
        document
            .check(&read(None), &plan, &mut visits, &|| Ok(()))
            .unwrap()
            .is_none()
    );
}
fn read(model: Option<&Value>) -> document::Expectation<'_> {
    document::Expectation {
        kind: document::Kind::Query,
        model,
        root_fields: None,
    }
}

#[test]
fn graphql_enum_match_requires_a_literal_or_a_variable_with_nonbuiltin_type() {
    let plan = policy::resolution::compiled::Plan::compile(vec![], None, json!({})).unwrap();
    let model = json!({"root":"query","states":{"query":{"types":["Query"],"fields":{"field":{"leaf":true,"arguments":{"order":{"required":true,"match":{"scalar":"enum"}}}}}}}});
    for (query, allowed) in [
        ("{field(order:OPEN)}", true),
        ("{field(order:\"OPEN\")}", false),
        ("query($order:Order=OPEN){field(order:$order)}", true),
        ("query($order:String=\"OPEN\"){field(order:$order)}", false),
        ("query($order:ID=\"OPEN\"){field(order:$order)}", false),
    ] {
        let carrier = carrier::decode(&request(query).arguments, None).unwrap();
        let mut visits = 0;
        let document = document::parse(&carrier, &mut visits, &|| Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(
            document
                .check(&read(Some(&model)), &plan, &mut visits, &|| Ok(()))
                .unwrap()
                .is_none(),
            allowed,
            "{query}"
        );
    }
}

#[test]
fn graphql_directives_and_operation_names_preserve_scope_in_source_and_compiled_plans() {
    let rules = custom();
    let plan =
        policy::resolution::compiled::Plan::compile(vec![], Some(custom()), json!({})).unwrap();
    let plan =
        policy::resolution::compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
    let read = r#"query Read { repository(owner:"acme",name:"web") { nameWithOwner } } mutation Write { field }"#;
    for (query, operation, expected) in [
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner @skip(if:true)}}"#,
            None,
            Decision::Allow,
        ),
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner @include(if:false)}}"#,
            None,
            Decision::Allow,
        ),
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner} viewer @skip(if:true){login}}"#,
            None,
            Decision::Deny,
        ),
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner} viewer @include(if:false){login}}"#,
            None,
            Decision::Deny,
        ),
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner} ...Hidden @skip(if:true)} fragment Hidden on Query {viewer{login}}"#,
            None,
            Decision::Deny,
        ),
        (
            r#"{repository(owner:"acme",name:"web"){nameWithOwner} ...Hidden @include(if:false)} fragment Hidden on Query {viewer{login}}"#,
            None,
            Decision::Deny,
        ),
        (
            r#"query($hide:Boolean=true){repository(owner:"acme",name:"web"){nameWithOwner} viewer @skip(if:$hide){login}}"#,
            None,
            Decision::Deny,
        ),
        (
            r#"query($show:Boolean=false){repository(owner:"acme",name:"web"){nameWithOwner} viewer @include(if:$show){login}}"#,
            None,
            Decision::Deny,
        ),
        (read, Some("Read"), Decision::Allow),
        (read, Some("Write"), Decision::Deny),
        (read, Some("Missing"), Decision::Deny),
        (read, None, Decision::Deny),
        (
            r#"query Read {repository(owner:"acme",name:"web"){nameWithOwner}} query Read {viewer{login}}"#,
            Some("Read"),
            Decision::Deny,
        ),
    ] {
        let mut request = request(query);
        if let Some(operation) = operation {
            request
                .arguments
                .extend(["-f".into(), format!("operationName={operation}")]);
        }
        let reference =
            policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &request).unwrap();
        let compiled = plan.evaluate(&request, &|| Ok(()), true, true).unwrap();
        assert_eq!(
            reference.decision, expected,
            "{query}; operation={operation:?}"
        );
        assert_eq!(
            json!(compiled),
            json!(reference),
            "{query}; operation={operation:?}"
        );
    }
}

#[test]
fn graphql_host_selection_uses_supplied_context_and_explicit_host_precedence() {
    let plan =
        policy::resolution::compiled::Plan::compile(vec![], Some(custom()), json!({})).unwrap();
    for (host, explicit, expected) in [
        (None, None, Decision::Allow),
        (Some("github.com"), None, Decision::Allow),
        (Some("git.example.com"), None, Decision::Deny),
        (Some("git.example.com"), Some("github.com"), Decision::Allow),
        (Some("github.com"), Some("git.example.com"), Decision::Deny),
    ] {
        let mut request = request(r#"{repository(owner:"acme",name:"web"){nameWithOwner}}"#);
        if let Some(host) = explicit {
            request.arguments.extend(["--hostname".into(), host.into()]);
        }
        let input = policy::resolution::normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: json!({}),
        };
        let context = policy::resolution::normalized::Context {
            cwd: request.cwd.clone(),
            working_files: None,
            environment: host
                .map(|host| std::collections::BTreeMap::from([("GH_HOST".into(), host.into())])),
        };
        let facts = policy::rules::Facts {
            input: &input,
            context: &context,
            executable: None,
            now: "2026-11-06T00:00:00Z",
            known_cwd: true,
            known_arguments: true,
            locations: None,
            container_read: None,
            effects: None,
        };
        assert_eq!(
            plan.evaluate_input(&request, &facts, &|| Ok(()))
                .unwrap()
                .decision,
            expected,
            "GH_HOST={host:?}; --hostname={explicit:?}"
        );
    }
}

fn mutation_model() -> Value {
    let input = |id: &str| {
        json!({"input":{"required":true,"match":{"object":{
            id:{"required":true,"match":{"scalar":"string"}},
            "clientMutationId":{"match":{"scalar":"string"}}
        }}}})
    };
    json!({"root":"mutation","states":{
        "mutation":{"types":["Mutation"],"fields":{
            "resolveReviewThread":{"arguments":input("threadId"),"select":"threadPayload"},
            "unresolveReviewThread":{"arguments":input("threadId"),"select":"threadPayload"},
            "mergePullRequest":{"arguments":input("pullRequestId"),"select":"payload"},
            "deletePullRequestReviewComment":{"arguments":input("id"),"select":"payload"}
        }},
        "threadPayload":{"types":["ResolveReviewThreadPayload","UnresolveReviewThreadPayload"],"fields":{
            "clientMutationId":{"leaf":true},"thread":{"select":"thread"}
        }},
        "thread":{"types":["PullRequestReviewThread"],"fields":{"id":{"leaf":true},"isResolved":{"leaf":true}}},
        "payload":{"types":["MergePullRequestPayload","DeletePullRequestReviewCommentPayload"],"fields":{"clientMutationId":{"leaf":true}}}
    }})
}
fn mutation_rule(id: &str, fields: &[&str], decision: &str) -> Value {
    json!({"id":id,"conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]},
        "graphql":{"carrier":"gh-api","operation":"mutation","root_fields":fields,"selection":mutation_model()}},
        "decision":decision})
}
fn mutation_rules() -> CustomRules {
    CustomRules::from_yaml(&json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","graphql-v1","graphql-mutations-v1"],
        "rules":[
            mutation_rule("threads", &["resolveReviewThread", "unresolveReviewThread"], "allow"),
            mutation_rule("merges", &["mergePullRequest"], "ask"),
            mutation_rule("deletes", &["deletePullRequestReviewComment"], "deny"),
            {"id":"other-api","conditions":{"domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},"decision":"deny"}
        ]
    }).to_string()).unwrap()
}

#[test]
fn graphql_mutations_take_the_strictest_listed_class_in_source_and_compiled_plans() {
    let rules = mutation_rules();
    let plan =
        policy::resolution::compiled::Plan::compile(vec![], Some(mutation_rules()), json!({}))
            .unwrap();
    // Full loading validates each rule on a copy whose model is a prepared reference.
    let plan = policy::resolution::compiled::Plan::load(plan.bytes().to_vec()).unwrap();
    let resolve = |id: &str| {
        format!(r#"resolveReviewThread(input:{{threadId:"{id}"}}){{thread{{id isResolved}}}}"#)
    };
    let merge = r#"mergePullRequest(input:{pullRequestId:"P1"}){clientMutationId}"#;
    for (query, extra, expected) in [
        (format!("mutation{{{}}}", resolve("T1")), vec![], Decision::Allow),
        (format!("mutation{{a:{} b:{}}}", resolve("T1"), resolve("T2")), vec![], Decision::Allow),
        (r#"mutation{resolveReviewThread(input:{threadId:"threadId: T1"}){thread{id}}}"#.into(), vec![], Decision::Allow),
        (
            r#"mutation($id:ID!){resolveReviewThread(input:{threadId:$id,clientMutationId:"c"}){clientMutationId}}"#.into(),
            vec!["-f", "id=T1"],
            Decision::Allow,
        ),
        (format!("mutation{{{} {merge}}}", resolve("T1")), vec![], Decision::Ask),
        (
            format!(r#"mutation{{{} deletePullRequestReviewComment(input:{{id:"C1"}}){{clientMutationId}}}}"#, resolve("T1")),
            vec![],
            Decision::Deny,
        ),
        // Aliases never stand in for actual names; conditionals and fragments still count.
        (format!("mutation{{resolveReviewThread:{merge}}}"), vec![], Decision::Ask),
        (
            format!("mutation($skip:Boolean=true){{{} mergePullRequest(input:{{pullRequestId:\"P1\"}}) @skip(if:$skip){{clientMutationId}}}}", resolve("T1")),
            vec![],
            Decision::Ask,
        ),
        (format!("mutation{{{} ...M}} fragment M on Mutation{{{merge}}}", resolve("T1")), vec![], Decision::Ask),
        // Everything below leaves the reviewed model and falls to the broad rule.
        (format!("mutation{{{} addStar(input:{{starrableId:\"R\"}}){{clientMutationId}}}}", resolve("T1")), vec![], Decision::Deny),
        (r#"mutation{resolveReviewThread(input:{threadId:"T1",extra:1}){thread{id}}}"#.into(), vec![], Decision::Deny),
        (r#"mutation{resolveReviewThread(input:{threadId:"T1",threadId:"T2"}){thread{id}}}"#.into(), vec![], Decision::Deny),
        // The parser skips a byte-order mark like whitespace, so it cannot hide a key.
        ("mutation{resolveReviewThread(input:{threadId:\"T1\",threadId\u{feff}:\"T2\"}){thread{id}}}".into(), vec![], Decision::Deny),
        (r#"mutation{resolveReviewThread(input:{threadId:["T1"]}){thread{id}}}"#.into(), vec![], Decision::Deny),
        (r#"mutation{resolveReviewThread(input:{clientMutationId:"c"}){thread{id}}}"#.into(), vec![], Decision::Deny),
        (
            "mutation($input:ResolveReviewThreadInput!){resolveReviewThread(input:$input){thread{id}}}".into(),
            vec!["-f", "input=T1"],
            Decision::Deny,
        ),
        (
            r#"mutation{resolveReviewThread(input:{threadId:"T1"}){thread{id pullRequest{repository{nameWithOwner}}}}}"#.into(),
            vec![],
            Decision::Deny,
        ),
        (format!("mutation{{{}}}", resolve("T1")), vec!["--paginate"], Decision::Deny),
        (r#"{repository(owner:"acme",name:"web"){nameWithOwner}}"#.into(), vec![], Decision::Deny),
    ] {
        let mut request = request(&query);
        request.arguments.extend(extra.iter().map(|a| (*a).to_owned()));
        let reference =
            policy::evaluate(&[], Some(&rules), policy::Profile::Recommended, &request).unwrap();
        assert_eq!(reference.decision, expected, "{query} {extra:?}");
        let compiled = plan.evaluate(&request, &|| Ok(()), true, true).unwrap();
        assert_eq!(json!(compiled), json!(reference), "{query} {extra:?}");
    }
}

#[test]
fn graphql_mutation_admission_requires_its_capability_classes_and_root_type() {
    let valid = json!({"carrier":"gh-api","operation":"mutation","root_fields":["mergePullRequest"],"selection":mutation_model()});
    assert!(validate(&valid).is_ok());
    let mut missing_fields = valid.clone();
    missing_fields
        .as_object_mut()
        .unwrap()
        .remove("root_fields");
    let mut missing_model = valid.clone();
    missing_model.as_object_mut().unwrap().remove("selection");
    let mut duplicate_fields = valid.clone();
    duplicate_fields["root_fields"] = json!(["mergePullRequest", "mergePullRequest"]);
    let mut query_root = valid.clone();
    query_root["selection"]["states"]["mutation"]["types"] = json!(["Query"]);
    let mut cursor = valid.clone();
    cursor["selection"]["states"]["mutation"]["fields"]["mergePullRequest"]["arguments"]["input"]
        ["match"]["object"]["after"] =
        json!({"nullable":true,"cursor":true,"match":{"scalar":"string"}});
    let mut deep = json!({"leaf":{"match":{"scalar":"string"}}});
    for _ in 0..5 {
        deep = json!({"nested":{"match":{"object":deep}}});
    }
    let mut too_deep = valid.clone();
    too_deep["selection"]["states"]["mutation"]["fields"]["mergePullRequest"]["arguments"] = deep;
    let mut query_fields = json!({"carrier":"gh-api","operation":"query","selection":model()});
    query_fields["root_fields"] = json!(["repository"]);
    let mut query_object = json!({"carrier":"gh-api","operation":"query","selection":model()});
    query_object["selection"]["states"]["query"]["fields"]["repository"]["arguments"]["filter"] =
        json!({"match":{"object":{"state":{"match":{"scalar":"enum"}}}}});
    let mut query_mutation_root =
        json!({"carrier":"gh-api","operation":"query","selection":mutation_model()});
    query_mutation_root
        .as_object_mut()
        .unwrap()
        .remove("root_fields");
    for invalid in [
        missing_fields,
        missing_model,
        duplicate_fields,
        query_root,
        cursor,
        too_deep,
        query_fields,
        query_object,
        query_mutation_root,
    ] {
        assert!(validate(&invalid).is_err(), "{invalid}");
    }
    let undeclared = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","graphql-v1"],
        "rules":[mutation_rule("threads", &["resolveReviewThread"], "allow")]});
    assert!(CustomRules::from_yaml(&undeclared.to_string()).is_err());
}
