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
    let plan = policy::compiled::Plan::compile(vec![], Some(rules), json!({})).unwrap();
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
    let plan = policy::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    let loaded = policy::compiled::Plan::load(plan.bytes().to_vec()).unwrap();
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
    let plan = policy::compiled::Plan::compile(
        vec![(pack(), policy::Profile::Recommended)],
        None,
        json!({}),
    )
    .unwrap();
    let plan = policy::compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
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
    let plan = policy::compiled::Plan::compile(
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
    let plan = policy::compiled::Plan::compile(
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
    let plan = policy::compiled::Plan::compile(vec![], None, json!({})).unwrap();
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
                    document.check(None, &plan, &mut visits, &|| Ok(())),
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
            .check(None, &plan, &mut visits, &|| Ok(()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn graphql_enum_match_requires_a_literal_or_a_variable_with_nonbuiltin_type() {
    let plan = policy::compiled::Plan::compile(vec![], None, json!({})).unwrap();
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
                .check(Some(&model), &plan, &mut visits, &|| Ok(()))
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
    let plan = policy::compiled::Plan::compile(vec![], Some(custom()), json!({})).unwrap();
    let plan = policy::compiled::Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
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
    let plan = policy::compiled::Plan::compile(vec![], Some(custom()), json!({})).unwrap();
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
        let input = policy::normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: json!({}),
        };
        let context = policy::normalized::Context {
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
