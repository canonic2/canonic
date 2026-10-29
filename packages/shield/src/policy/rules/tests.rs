//! Behavioral admission, predicate, unmet-branch and prepared-plan acceptance.
use super::*;
use crate::{
    configuration,
    policy::{CustomRules, Pack, Request, Scope, compiled::Plan},
};

fn rule(arguments: Value) -> Value {
    let mut row = json!({"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme"}});
    row["conditions"]["arguments"] = arguments;
    row
}
fn custom(rows: Vec<Value>) -> CustomRules {
    let mut source = document(&rows).unwrap();
    source["rules"] = Value::Array(rows);
    CustomRules::from_yaml(&source.to_string()).unwrap()
}
fn request(args: &[&str]) -> Request {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Request {
        executable: "acme".into(),
        arguments: args.iter().map(|s| (*s).into()).collect(),
        cwd: cwd.clone(),
        project: cwd,
    }
}

#[test]
fn prefix_leading_matches_operations_without_scanning_values_or_suffixes() {
    let row = json!({"id":"administration","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["login"],"leading":{"flags":["-q"],"values":[{"name":"--format","match":{"one_of":["text","login"]},"repeat":"forbid"}]}}}});
    let source = document(std::slice::from_ref(&row)).unwrap();
    assert!(
        source["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("prefix-leading-v1"))
    );
    let evaluator = prepared(vec![row.clone()]);
    let restored =
        configuration::Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec())
            .unwrap();
    for (args, expected) in [
        (vec!["login"], Decision::Deny),
        (
            vec!["-qq", "--format=text", "login", "--unknown"],
            Decision::Deny,
        ),
        (vec!["--", "login", "--unknown"], Decision::Deny),
        (vec!["--format", "login", "test", "login"], Decision::Ask),
        (vec!["test", "login"], Decision::Ask),
        (vec!["--format", "login"], Decision::Ask),
        (vec!["--format=invalid", "login"], Decision::Ask),
        (
            vec!["--format=text", "--format=text", "login"],
            Decision::Ask,
        ),
        (vec!["--unknown", "login"], Decision::Ask),
    ] {
        let req = request(&args);
        assert_eq!(
            crate::policy::evaluate(
                &[],
                Some(&custom(vec![row.clone()])),
                Profile::Recommended,
                &req,
            )
            .unwrap()
            .decision,
            expected,
            "reference {args:?}"
        );
        for candidate in [&evaluator, &restored] {
            assert_eq!(
                candidate.evaluate(&req).unwrap().decision,
                expected,
                "{args:?}"
            );
        }
    }
    let mut missing = source;
    missing["capabilities"]
        .as_array_mut()
        .unwrap()
        .retain(|cap| cap != "prefix-leading-v1");
    assert!(CustomRules::from_yaml(&missing.to_string()).is_err());
    let mut invalid = row;
    invalid["conditions"]["arguments"]["leading"]["operands"] = json!(true);
    assert!(document(&[invalid]).is_err());
}
#[test]
fn argument_paths_validate_declared_values_and_do_not_claim_runtime_footprints() {
    let root = assert_fs::TempDir::new().unwrap();
    let root = root.path().canonicalize().unwrap();
    let grammar = json!({"kind":"options","prefix":["build"],"flags":["--locked"],"values":["--manifest-path","--target-dir","-o"],"operands":true});
    let inside = json!({"path":{"within":{"root":"project"},"exclude":[{"root":"project","path":".shield"}]}});
    let mut row = json!({"id":"explicit-paths","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requirements":{"argument_paths":{"grammar":grammar,"options":{"--manifest-path":inside,"--target-dir":{"path":{"within":{"root":"project"},"exclude":[{"root":"project","path":".shield"}]},"recursive":true},"-o":inside},"operands":inside}},"on_unmet":{"decision":"deny"}});
    let evaluator = prepared(vec![row.clone()]);
    let restored =
        configuration::Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec())
            .unwrap();
    let mut req = Request {
        executable: "acme".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    for (args, decision) in [
        (vec!["build", "--locked"], Decision::Allow),
        (
            vec![
                "build",
                "--manifest-path",
                "crate/Cargo.toml",
                "--target-dir=target",
                "-oout",
            ],
            Decision::Allow,
        ),
        (
            vec!["build", "--manifest-path=../Cargo.toml"],
            Decision::Deny,
        ),
        (
            vec!["build", "--manifest-path=in", "--manifest-path=../out"],
            Decision::Deny,
        ),
        (vec!["build", "--target-dir", "."], Decision::Deny),
        (vec!["build", "--", ".shield/config.yml"], Decision::Deny),
        (vec!["build", "--manifest-path"], Decision::Deny),
        (vec!["build", "--unknown", "file"], Decision::Deny),
        (vec!["build", "-"], Decision::Deny),
    ] {
        req.arguments = args.iter().map(|a| (*a).into()).collect();
        for candidate in [&evaluator, &restored] {
            assert_eq!(
                candidate.evaluate(&req).unwrap().decision,
                decision,
                "{args:?}"
            );
        }
    }
    // Explicit paths do not invent a complete resource model for this program.
    row["requirements"]["targets"] = json!({"action":"write","all":{"within":{"root":"project"}}});
    req.arguments = vec!["build".into()];
    assert_eq!(
        prepared(vec![row.clone()]).evaluate(&req).unwrap().decision,
        Decision::Deny
    );
    row["requirements"]
        .as_object_mut()
        .unwrap()
        .remove("targets");
    let mut exact = row.clone();
    exact["requirements"]["argument_paths"] = json!({"grammar":grammar,"options":{"-o":{"path":{"exact":{"root":"write","path":"out"}}}}});
    let exact = document(&[exact]).unwrap();
    assert!(
        exact["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("access-paths-v1"))
    );
    assert!(
        exact["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("location-v1"))
    );
    row["requirements"]["argument_paths"]["options"]["--unknown"] = inside;
    assert!(document(&[row]).is_err());
}

#[test]
#[cfg(unix)]
fn argument_paths_follow_symlinks_and_preserve_native_component_order() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir(outside.path().join("nested")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("nested"), root.join("escape")).unwrap();
    let evaluator = prepared(vec![
        json!({"id":"paths","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"requirements":{"argument_paths":{"grammar":{"kind":"options","prefix":[],"flags":[],"values":["--output"],"operands":true},"options":{"--output":{"path":{"within":{"root":"project"}}}},"operands":{"path":{"within":{"root":"project"}}}}},"on_unmet":{"decision":"deny"}}),
    ]);
    let mut req = Request {
        executable: "acme".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    for (args, expected) in [
        (vec!["src/new.rs"], Decision::Allow),
        (vec!["--output=escape/new.rs"], Decision::Deny),
        (vec!["escape/../new.rs"], Decision::Deny),
        (vec!["src/new.rs", "escape/read.rs"], Decision::Deny),
    ] {
        req.arguments = args.iter().map(|a| (*a).into()).collect();
        assert_eq!(
            evaluator.evaluate(&req).unwrap().decision,
            expected,
            "{args:?}"
        );
    }
}

fn evaluate(row: Value, args: &[&str]) -> Value {
    let c = custom(vec![row]);
    json!(crate::policy::evaluate(&[], Some(&c), Profile::Recommended, &request(args)).unwrap())
}
fn prepared(rows: Vec<Value>) -> configuration::Evaluator {
    let mut source = document(&rows).unwrap();
    source["rules"] = Value::Array(rows);
    let source = source.to_string();
    let snapshot = configuration::Candidate::from_yaml(
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
    )
    .unwrap()
    .capture(&[], Some(&source))
    .unwrap();
    configuration::Evaluator::new(vec![(Scope::Project, snapshot)]).unwrap()
}

#[test]
fn documents_require_metadata_and_refuse_unknown_fields_and_source_nulls() {
    for source in [
        r#"{"schema_version":1,"rules":[]}"#,
        r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[],"scope":"project"}"#,
    ] {
        assert!(CustomRules::from_yaml(source).is_err());
    }
    for field in [
        "requirements",
        "on_unmet",
        "guidance",
        "reason",
        "expires_at",
    ] {
        let mut row = rule(json!({"kind":"any"}));
        row[field] = Value::Null;
        assert!(document(&[row]).is_err(), "{field}");
    }
    let source = document(&[rule(json!({"kind":"any"}))]).unwrap();
    let mut missing = source.clone();
    missing["capabilities"] = json!(["structured-rules-v1"]);
    assert!(CustomRules::from_yaml(&missing.to_string()).is_err());
    let mut duplicate = source.clone();
    duplicate["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "argument-rules-v1"
    ]);
    assert!(CustomRules::from_yaml(&duplicate.to_string()).is_err());
    assert!(CustomRules::from_yaml("schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: &rules []\n").is_err());
    assert!(CustomRules::from_yaml(r#"{"schema_version":1,"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[]}"#).is_err());
}

#[test]
fn sequences_preserve_tokens_and_regex_boundaries() {
    let row = rule(
        json!({"kind":"sequence","prefix":[{"regex":"get|describe"},{"one_of":["json","text"]}],"rest":{"kind":"each","match":{"glob":"*.report"},"min":1,"max":2}}),
    );
    for (args, decision) in [
        (vec!["get", "json", "a.report"], "allow"),
        (vec!["describe", "text", ".report", "b.report"], "allow"),
        (vec!["get-more", "json", "a.report"], "ask"),
        (vec!["get json", "a.report"], "ask"),
        (vec!["get", "json"], "ask"),
        (vec!["get", "json", "nested/a.report"], "ask"),
    ] {
        assert_eq!(evaluate(row.clone(), &args)["decision"], decision);
    }
}

#[test]
fn requirements_emit_real_failures_and_candidate_only_guidance() {
    let row = json!({"id":"readonly","decision":"allow","conditions":{"domain":"argv","executable":"acme"},"requirements":{"arguments":{"kind":"options","prefix":[],"flags":["--read-only"],"values":["--user"],"operands":false,"require_all":["--read-only"]}},"on_unmet":{"decision":"deny","message":"Use the required read-only option.","alternatives":["Add --read-only and submit a revised request."]}});
    assert_eq!(evaluate(row.clone(), &["--read-only"])["decision"], "allow");
    for args in [
        &[][..],
        &["--user", "--read-only"][..],
        &["--", "--read-only"][..],
    ] {
        let result = evaluate(row.clone(), args);
        assert_eq!(result["decision"], "deny");
        assert!(
            result["requirement_checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == "required_option_missing")
        );
        assert_eq!(result["guidance"][0]["candidate_only"], true);
        assert_eq!(result["matches"][0]["branch"], "unmet");
    }
    let mut out_of_scope = request(&[]);
    out_of_scope.executable = "other".into();
    let c = custom(vec![row]);
    let result =
        json!(crate::policy::evaluate(&[], Some(&c), Profile::Recommended, &out_of_scope).unwrap());
    assert_eq!(result["decision"], "ask");
    assert_eq!(result["guidance"], json!([]));
}

#[test]
fn options_have_explicit_arity_repetition_phases_and_separators() {
    let options = json!({"kind":"options","leading":{"flags":["-v"],"values":["--region"]},"prefix":["inspect"],"flags":["-a","-b"],"values":[{"name":"--format","match":{"one_of":["json","text"]},"repeat":"forbid"},"-o"],"optional_values":["--color"],"operands":false,"require_all":["--format"]});
    let row = rule(options);
    for args in [
        vec![
            "-v",
            "--region",
            "ca",
            "inspect",
            "-ab",
            "--format=json",
            "-ovalue",
        ],
        vec!["inspect", "--format", "text", "--color"],
        vec!["inspect", "--format=text", "--color=always"],
    ] {
        assert_eq!(evaluate(row.clone(), &args)["decision"], "allow");
    }
    for args in [
        vec!["inspect", "--format=json", "--format=text"],
        vec!["inspect", "--format=xml"],
        vec!["--unknown", "inspect", "--format=json"],
        vec!["inspect", "--format=json", "--", "-a"],
        vec!["inspect", "--format=json", "--color", "always"],
    ] {
        assert_eq!(evaluate(row.clone(), &args)["decision"], "ask");
    }
    let store = crate::policy::patterns::Store::default();
    let parsed = argument_rules::matches(
        &row["conditions"]["arguments"],
        &["-v", "inspect", "--format=json"].map(String::from),
        &store,
    )
    .unwrap();
    assert_eq!((parsed.records[0].index, parsed.records[0].phase), (0, 0));
    assert_eq!((parsed.records[1].index, parsed.records[1].phase), (2, 1));
}

#[test]
fn prepared_patterns_match_unicode_boundaries_and_glob_byte_semantics() {
    for (selector, value, allowed) in [
        (json!({"regex":r"\b\p{L}+\b"}), "Éclair", true),
        (json!({"regex":"(?m:get|describe)"}), "get\ndescribe", false),
        (json!({"glob":"?"}), "é", false),
        (json!({"glob":"??"}), "é", true),
        (json!({"glob":"**/report"}), "a/b/report", true),
        (json!({"glob":"**/report"}), "report", true),
        (json!({"glob":"**"}), "a/b/report", true),
        (json!({"glob":"dir/**"}), "dir/a/b", true),
        (json!({"glob":"dir/**"}), "dir", false),
        (json!({"glob":"dir/**/report"}), "dir/report", true),
        (json!({"glob":"dir/**/report"}), "dir/a/report", true),
        (json!({"glob":"{**/report,notes}"}), "a/report", true),
        (json!({"glob":"{dir/**,notes}"}), "dir/a/b", true),
        (json!({"glob":"{[ab],c}"}), "a", true),
        (json!({"glob":"{a,[bc]}"}), "c", true),
        (json!({"glob":"{[]**],c}"}), "*", true),
        (json!({"glob":"{[!]*],c}"}), "a", true),
        (json!({"glob":"[*][*]"}), "**", true),
    ] {
        let row = rule(json!({"kind":"sequence","prefix":[selector],"rest":{"kind":"none"}}));
        let c = custom(vec![row]);
        let expected =
            json!(crate::policy::evaluate_selected(&[], &[&c], &request(&[value])).unwrap());
        let plan = Plan::compile(vec![], Some(c), json!({})).unwrap();
        assert_eq!(&plan.bytes()[..8], b"SHPLAN08");
        let restored = Plan::load_authenticated(plan.bytes().to_vec()).unwrap();
        let actual = json!(
            restored
                .evaluate(&request(&[value]), &|| Ok(()), true, true)
                .unwrap()
        );
        assert_eq!(actual, expected);
        assert_eq!(actual["decision"], if allowed { "allow" } else { "ask" });
    }
    for source in [
        "a{,b}",
        "a{b,}",
        "[unclosed",
        "{a,{b,c}}",
        "{[ab],}",
        "{,[ab]}",
        "a**b",
        "***",
        "***/report",
        "dir/***",
        "dir/**suffix",
        "prefix**/report",
        "{a**b,c}",
        "{**,c}",
        "{a,**}",
        "{[]**],}",
    ] {
        let row =
            rule(json!({"kind":"sequence","prefix":[{"glob":source}],"rest":{"kind":"none"}}));
        assert!(CustomRules::from_yaml(&document(&[row]).unwrap().to_string()).is_err());
    }
    // Unbalanced groups would escape the full-field anchors once wrapped.
    for source in ["(?=x)", r"(x)\1", "x)|(?s:.*", "a)|(b", ")(", "x)|.*|(y"] {
        let row =
            rule(json!({"kind":"sequence","prefix":[{"regex":source}],"rest":{"kind":"none"}}));
        assert!(CustomRules::from_yaml(&document(&[row]).unwrap().to_string()).is_err());
    }
}

#[test]
fn cache_scope_index_keeps_unmet_arguments() {
    let row = json!({"id":"required","decision":"allow","conditions":{"domain":"argv","executable":"acme"},"requirements":{"arguments":{"kind":"exact","values":["safe"]}},"on_unmet":{"decision":"deny","message":"Use the exact safe operation."}});
    let evaluator = prepared(vec![row]);
    assert_eq!(
        evaluator.evaluate(&request(&["unsafe"])).unwrap().decision,
        Decision::Deny
    );
    assert_eq!(
        evaluator.evaluate(&request(&["safe"])).unwrap().decision,
        Decision::Allow
    );
    let restored =
        configuration::Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec())
            .unwrap();
    assert_eq!(
        restored.evaluate(&request(&["unsafe"])).unwrap().decision,
        Decision::Deny
    );
}

#[test]
fn expiry_uses_actual_instants_and_clock_failure_blocks() {
    assert!(timestamp("2028-02-29T12:00:00.123456789Z").is_some());
    for invalid in [
        "0000-02-29T12:00:00Z",
        "2027-02-29T12:00:00Z",
        "2028-02-29T12:00:60Z",
        "2028-02-29T12:00:00.Z",
    ] {
        assert!(timestamp(invalid).is_none());
    }
    let mut row = rule(json!({"kind":"any"}));
    row["expires_at"] = json!("2000-01-01T00:00:00.1Z");
    assert_eq!(evaluate(row, &[])["decision"], "ask");
}

#[test]
fn typed_mcp_fields_distinguish_missing_null_and_unlisted_fields() {
    let row = json!({"id":"publish","decision":"allow","conditions":{"domain":"mcp","server":{"glob":"acme-*"},"tool":"publish"},"requirements":{"fields":{"/force":{"exact":false},"/label":{"text":{"regex":"[a-z]+"}}}},"on_unmet":{"decision":"deny","message":"Use ordinary publication with a lowercase label."}});
    let evaluator = prepared(vec![row]);
    let context = Context {
        cwd: request(&[]).cwd,
        environment: None,
        working_files: None,
    };
    for (args, decision) in [
        (
            json!({"force":false,"label":"release","extra":true}),
            "allow",
        ),
        (json!({"force":null,"label":"release"}), "deny"),
        (json!({"label":"release"}), "deny"),
        (json!({"force":false,"label":"Release"}), "deny"),
    ] {
        let input = Input::Mcp {
            server: "acme-publish".into(),
            tool: "publish".into(),
            arguments: args,
            connection: None,
            original_input: json!({}),
        };
        let result = evaluator
            .evaluate_normalized(
                &input,
                &context,
                &context.cwd,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(result["decision"], decision);
    }
}

#[test]
fn mcp_field_evidence_omits_sensitive_expected_values() {
    let expected = "tok-acme-sensitive-fixture";
    let context = Context {
        cwd: request(&[]).cwd,
        environment: None,
        working_files: None,
    };
    for constraint in [
        json!({"exact":expected}),
        json!({"text":{"exact":expected}}),
    ] {
        for (pointer, arguments, sensitive) in [
            (
                "/credentials/token",
                json!({"credentials":{"token":"different"}}),
                true,
            ),
            (
                "/credentials/password",
                json!({"credentials":{"password":"different"}}),
                true,
            ),
            ("/token/0", json!({"token":["different"]}), true),
            (
                "/credential~1store/token",
                json!({"credential/store":{"token":"different"}}),
                true,
            ),
            (
                "/credentials/region",
                json!({"credentials":{"region":"different"}}),
                false,
            ),
        ] {
            let row = json!({"id":"field","decision":"allow",
                "conditions":{"domain":"mcp","server":"acme","tool":"inspect"},
                "requirements":{"fields":{pointer:constraint}},"on_unmet":{"decision":"deny"}});
            let input = Input::Mcp {
                server: "acme".into(),
                tool: "inspect".into(),
                arguments,
                connection: None,
                original_input: json!({}),
            };
            let result = prepared(vec![row])
                .evaluate_normalized(
                    &input,
                    &context,
                    &context.cwd,
                    None,
                    &crate::evidence::now(),
                    &|| Ok(()),
                )
                .unwrap();
            assert_eq!(result["decision"], "deny");
            let field = &result["explanation"]["requirement_checks"][0]["evidence"]["fields"][0];
            assert_eq!(field["pointer"], pointer);
            assert_eq!(field.get("expected").is_none(), sensitive, "{result}");
            if sensitive {
                assert!(!result.to_string().contains(expected), "{result}");
                let message = crate::agent_message::detail(&result["explanation"]).unwrap();
                assert!(message.contains(pointer), "{message}");
                assert!(!message.contains(expected), "{message}");
            } else {
                assert_eq!(field["expected"], constraint);
            }
        }
    }
    // This spelling is already rejected at admission; slash-separated terminal
    // names above were admitted and need evidence-level protection as well.
    let rejected: Rule = serde_json::from_value(json!({"id":"field","decision":"allow",
        "conditions":{"domain":"mcp","server":"acme","tool":"inspect"},
        "requirements":{"fields":{"/effective/env/API_TOKEN":{"exact":expected}}},
        "on_unmet":{"decision":"deny"}}))
    .unwrap();
    assert!(rejected.validate().is_err());
}

fn evaluate_argv(evaluator: &configuration::Evaluator, args: &[&str], context: &Context) -> Value {
    let input = Input::Argv {
        executable: "acme".into(),
        arguments: args.iter().map(|s| (*s).into()).collect(),
        original_input: json!({}),
    };
    evaluator
        .evaluate_normalized(
            &input,
            context,
            &context.cwd,
            None,
            &crate::evidence::now(),
            &|| Ok(()),
        )
        .unwrap()
}

#[test]
fn met_branch_ranks_exact_context_split_across_conditions_and_requirements() {
    let exact = json!({"kind":"exact","values":["read"]});
    let mut scoped = rule(exact.clone());
    scoped["id"] = json!("scoped");
    scoped["requirements"] =
        json!({"cwd":{"exact":{"root":"project"}},"environment":{"exact":{"ACME":"1"}}});
    scoped["on_unmet"] = json!({"decision":"ask"});
    let mut broad = rule(exact);
    broad["id"] = json!("broad");
    broad["decision"] = json!("deny");
    let evaluator = prepared(vec![scoped, broad]);
    let mut context = Context {
        cwd: request(&[]).cwd,
        environment: Some(std::collections::BTreeMap::from([(
            "ACME".into(),
            "1".into(),
        )])),
        working_files: None,
    };
    assert_eq!(
        evaluate_argv(&evaluator, &["read"], &context)["decision"],
        "allow"
    );
    context.environment = Some(std::collections::BTreeMap::new());
    assert_eq!(
        evaluate_argv(&evaluator, &["read"], &context)["decision"],
        "deny"
    );
}

#[test]
fn environment_variable_names_are_data_not_predicate_keys() {
    let context = |name: &str, value: &str| Context {
        cwd: request(&[]).cwd,
        environment: Some(std::collections::BTreeMap::from([(
            name.into(),
            value.into(),
        )])),
        working_files: None,
    };
    for (name, selector, capability, matching, other) in [
        ("regex", json!("a("), None, "a(", "a"),
        ("glob", json!("*"), None, "*", "a"),
        (
            "exact",
            json!({"glob":"acme-*"}),
            Some("glob-v1"),
            "acme-1",
            "other",
        ),
        (
            "context",
            json!({"regex":"^v[0-9]$"}),
            Some("regex-v1"),
            "v1",
            "v10",
        ),
    ] {
        let mut row = rule(json!({"kind":"exact","values":["read"]}));
        row["conditions"]["environment"] = json!({"fields":{name:selector}});
        let capabilities = document(std::slice::from_ref(&row)).unwrap()["capabilities"].clone();
        for pattern in ["regex-v1", "glob-v1"] {
            assert_eq!(
                capabilities.as_array().unwrap().contains(&json!(pattern)),
                capability == Some(pattern),
                "{name}: {capabilities}"
            );
        }
        let evaluator = prepared(vec![row]);
        for (value, decision) in [(matching, "allow"), (other, "ask")] {
            assert_eq!(
                evaluate_argv(&evaluator, &["read"], &context(name, value))["decision"],
                decision,
                "{name}={value}"
            );
        }
    }
}

#[test]
fn web_rules_parse_url_authority_and_typed_fields() {
    let row = json!({"id":"reports","decision":"allow","conditions":{"domain":"web","action":"fetch"},"requirements":{"url":{"schemes":["https"],"hosts":["api.example.com"],"ports":[443],"path":{"prefix":"/reports/"}},"method":"GET"},"on_unmet":{"decision":"deny"}});
    for (field, empty) in [
        ("ports", json!([])),
        ("hosts", json!([])),
        ("schemes", json!([])),
    ] {
        let mut invalid = row.clone();
        invalid["requirements"]["url"][field] = empty;
        assert!(document(&[invalid]).is_err(), "empty {field}");
    }
    let evaluator = prepared(vec![row]);
    let context = Context {
        cwd: request(&[]).cwd,
        environment: None,
        working_files: None,
    };
    for (url, method, decision) in [
        ("https://API.EXAMPLE.COM/reports/today", "GET", "allow"),
        (
            "https://api.example.com.evil.test/reports/today",
            "GET",
            "deny",
        ),
        ("https://api.example.com:8443/reports/today", "GET", "deny"),
        ("https://api.example.com/reports/today", "POST", "deny"),
        ("https://api.example.com/a%2Freports/today", "GET", "deny"),
    ] {
        let input = Input::Web {
            action: "fetch".into(),
            url: Some(url.into()),
            query: None,
            method: Some(method.into()),
            conditions: json!({}),
            original_input: json!({}),
        };
        assert_eq!(
            evaluator
                .evaluate_normalized(
                    &input,
                    &context,
                    &context.cwd,
                    None,
                    &crate::evidence::now(),
                    &|| Ok(())
                )
                .unwrap()["decision"],
            decision
        );
    }
}

#[test]
fn pack_profiles_cover_both_outcomes_and_keep_custom_authority() {
    let row = json!({"id":"required","decision":{"loose":"allow","recommended":"ask","strict":"deny"},"conditions":{"domain":"argv","executable":"acme"},"requirements":{"arguments":{"kind":"exact","values":["inspect"]}},"on_unmet":{"decision":{"loose":"ask","recommended":"deny","strict":"deny"}}});
    let source = json!({"schema_version":1,"kind":"pack","id":"acme/inspect","version":"1.0.0","description":"Acme inspection","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1","requirements-v1"],"rules":[row]});
    let pack = Pack::from_yaml(&source.to_string()).unwrap();
    let mut scalar = source.clone();
    scalar["rules"][0]["decision"] = json!("allow");
    scalar["rules"][0]["on_unmet"]["decision"] = json!("deny");
    let scalar = Pack::from_yaml(&scalar.to_string()).unwrap();
    for profile in [Profile::Loose, Profile::Recommended, Profile::Strict] {
        for (argument, expected) in [("inspect", Decision::Allow), ("write", Decision::Deny)] {
            assert_eq!(
                crate::policy::evaluate(
                    std::slice::from_ref(&scalar),
                    None,
                    profile,
                    &request(&[argument])
                )
                .unwrap()
                .decision,
                expected
            );
        }
    }
    let mut invalid = source.clone();
    invalid["rules"][0]["decision"] = json!({"loose":"allow","recommended":"ask"});
    assert!(Pack::from_yaml(&invalid.to_string()).is_err());
    invalid["rules"][0]["decision"] = json!("allow");
    invalid["rules"][0]["on_unmet"]["decision"] = json!("allow");
    assert!(Pack::from_yaml(&invalid.to_string()).is_err());
    for (profile, met, unmet) in [
        (Profile::Loose, Decision::Allow, Decision::Ask),
        (Profile::Recommended, Decision::Ask, Decision::Deny),
        (Profile::Strict, Decision::Deny, Decision::Deny),
    ] {
        assert_eq!(
            crate::policy::evaluate(
                std::slice::from_ref(&pack),
                None,
                profile,
                &request(&["inspect"])
            )
            .unwrap()
            .decision,
            met
        );
        assert_eq!(
            crate::policy::evaluate(
                std::slice::from_ref(&pack),
                None,
                profile,
                &request(&["write"])
            )
            .unwrap()
            .decision,
            unmet
        );
    }
    let override_rule = custom(vec![rule(json!({"kind":"exact","values":["write"]}))]);
    assert_eq!(
        crate::policy::evaluate(
            &[pack],
            Some(&override_rule),
            Profile::Strict,
            &request(&["write"])
        )
        .unwrap()
        .decision,
        Decision::Allow
    );
}

#[test]
#[cfg(unix)]
fn search_models_check_paths_and_reject_explicit_helpers() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("item"), "Acme").unwrap();
    std::fs::create_dir(root.join(".aws")).unwrap();
    std::fs::write(root.join(".aws/credentials"), "Acme").unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::write(outside.path().join("item"), "Acme").unwrap();
    std::os::unix::fs::symlink(outside.path().join("item"), root.join("escape")).unwrap();
    let evaluator = prepared(vec![
        json!({"id":"search","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["grep","rg","/usr/bin/grep"]},"arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":".aws"}]}}},"on_unmet":{"decision":"deny"}}),
    ]);
    let mut req = Request {
        executable: "grep".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    for (executable, arguments, decision) in [
        ("grep", vec!["-nF", "Acme", "item"], Decision::Allow),
        (
            "/usr/bin/grep",
            vec!["--", "-Acme", "item"],
            Decision::Allow,
        ),
        ("grep", vec!["Acme", "item", "escape"], Decision::Deny),
        ("grep", vec!["Acme", ".aws/credentials"], Decision::Deny),
        ("grep", vec!["Acme", "."], Decision::Deny),
        ("grep", vec!["-r", "Acme", "."], Decision::Deny),
        ("grep", vec!["-f", "item", "item"], Decision::Deny),
        ("grep", vec!["Acme"], Decision::Deny),
        ("grep", vec!["Acme", "item", "-f/outside"], Decision::Deny),
        (
            "rg",
            vec!["--no-config", "--no-ignore", "-n", "Acme", "item"],
            Decision::Allow,
        ),
        ("rg", vec!["Acme", "item"], Decision::Allow),
        // Config-free searches remain supported as an optional form.
        ("rg", vec!["--no-config", "Acme", "item"], Decision::Allow),
        // Without --no-ignore, an existing excluded directory blocks the walk.
        ("rg", vec!["--no-config", "Acme"], Decision::Deny),
        ("rg", vec!["--no-config", "--files"], Decision::Deny),
        // After -- this is a literal search pattern, not a required flag.
        ("rg", vec!["--", "--no-config", "item"], Decision::Allow),
        (
            "rg",
            vec!["--no-config", "--no-ignore", "--pre", "sh", "Acme", "item"],
            Decision::Deny,
        ),
        (
            "rg",
            vec!["--no-config", "--no-ignore", "Acme", "missing"],
            Decision::Deny,
        ),
    ] {
        req.executable = executable.into();
        req.arguments = arguments.into_iter().map(str::to_owned).collect();
        assert_eq!(
            evaluator.evaluate(&req).unwrap().decision,
            decision,
            "{req:?}"
        );
    }
}

#[test]
#[cfg(unix)]
fn ripgrep_visible_trees_preserve_exclusions_without_following_descendant_links() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".aws")).unwrap();
    std::fs::create_dir(root.join("private")).unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
    let policy = |excluded: &str| {
        prepared(vec![
            json!({"id":"tree","decision":"allow","conditions":{"domain":"argv","executable":"rg","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":excluded}]}}},"on_unmet":{"decision":"deny"}}),
        ])
    };
    let evaluator = policy(".aws");
    let mut req = Request {
        executable: "rg".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    for (args, decision) in [
        (vec!["Acme", "."], Decision::Allow),
        (vec!["Acme", ".aws"], Decision::Deny),
        (vec!["Acme", "escape"], Decision::Deny),
        (vec!["--hidden", "Acme", "."], Decision::Deny),
        (vec!["--follow", "Acme", "."], Decision::Deny),
        (vec!["-uu", "Acme", "."], Decision::Deny),
        (vec!["--glob", "*", "Acme", "."], Decision::Deny),
        (vec!["--ignore-file", "config", "Acme", "."], Decision::Deny),
    ] {
        req.arguments = ["--no-config", "--no-ignore"]
            .into_iter()
            .chain(args)
            .map(str::to_owned)
            .collect();
        assert_eq!(
            evaluator.evaluate(&req).unwrap().decision,
            decision,
            "{req:?}"
        );
    }
    req.arguments = vec![
        "--no-config".into(),
        "--no-ignore".into(),
        "Acme".into(),
        ".".into(),
    ];
    assert_eq!(
        policy("private").evaluate(&req).unwrap().decision,
        Decision::Deny
    );
    // Canonical aliases can expose a nominally hidden exclusion as a visible tree.
    std::fs::remove_dir(root.join(".aws")).unwrap();
    std::os::unix::fs::symlink(root.join("private"), root.join(".aws")).unwrap();
    assert_eq!(evaluator.evaluate(&req).unwrap().decision, Decision::Deny);
}

#[test]
#[cfg(unix)]
fn find_walks_check_present_exclusions_and_fail_actions() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src/nested")).unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
    let evaluator = prepared(vec![
        json!({"id":"walk","decision":"allow","conditions":{"domain":"argv","executable":"find","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":".aws"}]}}},"on_unmet":{"decision":"deny"}}),
    ]);
    let mut req = Request {
        executable: "find".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    let cases = |req: &mut Request, cases: &[(&[&str], Decision)]| {
        for (args, decision) in cases {
            req.arguments = args.iter().map(|&a| a.to_owned()).collect();
            assert_eq!(
                evaluator.evaluate(req).unwrap().decision,
                *decision,
                "{req:?}"
            );
        }
    };
    cases(
        &mut req,
        &[
            (&[], Decision::Allow),
            (&[".", "-name", "*.rs", "-type", "f"], Decision::Allow),
            (
                &["-P", "src", ".", "-maxdepth", "2", "-print0"],
                Decision::Allow,
            ),
            (&["src", "escape", "-print"], Decision::Deny),
            (
                &[
                    "src", "(", "-name", "a", "-o", "-iname", "b", ")", "!", "-empty",
                ],
                Decision::Allow,
            ),
            (
                &["src", "-type", "f", "-exec", "cat", "{}", ";"],
                Decision::Deny,
            ),
            (&["src", "-delete"], Decision::Deny),
            (&["src", "-fprint", "list"], Decision::Deny),
            (&["-L", "src"], Decision::Deny),
            (&["src", "-name"], Decision::Deny),
            (&["src", "-newer", "item"], Decision::Deny),
            (&["escape"], Decision::Deny),
        ],
    );
    // A walk reaches an exclusion only once it exists.
    std::fs::create_dir(root.join(".aws")).unwrap();
    cases(
        &mut req,
        &[
            (&["."], Decision::Deny),
            (&["src"], Decision::Allow),
            (&[".aws"], Decision::Deny),
        ],
    );
}

#[test]
#[cfg(unix)]
fn mkdir_and_touch_models_check_writes_and_fail_unknown_effects() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
    let evaluator = prepared(vec![
        json!({"id":"write","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["mkdir","touch","/bin/mkdir"]},"arguments":{"kind":"any"}},"requirements":{"targets":{"action":"write","all":{"within":{"root":"project"},"match":{"regex":".+"},"exclude":[{"root":"project","path":".git"}]}}},"on_unmet":{"decision":"deny"}}),
    ]);
    let mut req = Request {
        executable: "mkdir".into(),
        arguments: vec![],
        cwd: root.to_string_lossy().into_owned(),
        project: root.to_string_lossy().into_owned(),
    };
    for (executable, arguments, decision) in [
        ("mkdir", vec!["-p", "generated/nested"], Decision::Allow),
        ("/bin/mkdir", vec!["generated"], Decision::Allow),
        ("mkdir", vec!["."], Decision::Deny),
        ("mkdir", vec!["-p", ".git/new"], Decision::Deny),
        ("mkdir", vec!["-p", ".git/../generated"], Decision::Deny),
        ("mkdir", vec!["-m", "777", "generated"], Decision::Deny),
        ("mkdir", vec!["-p", "escape/new"], Decision::Deny),
        ("touch", vec!["-amc", "generated"], Decision::Allow),
        ("touch", vec!["--", "-generated"], Decision::Allow),
        ("touch", vec!["generated", "escape/new"], Decision::Deny),
        ("touch", vec![".git/config"], Decision::Deny),
        (
            "touch",
            vec!["-r", "reference", "generated"],
            Decision::Deny,
        ),
        ("touch", vec!["--", "-"], Decision::Deny),
        (
            "touch",
            vec!["generated", "--reference=/outside"],
            Decision::Deny,
        ),
        ("touch", vec![], Decision::Deny),
    ] {
        req.executable = executable.into();
        req.arguments = arguments.into_iter().map(str::to_owned).collect();
        assert_eq!(
            evaluator.evaluate(&req).unwrap().decision,
            decision,
            "{req:?}"
        );
    }
    assert!(!root.join("generated").exists());
}

#[test]
fn location_requirements_resolve_cwd_mixed_targets_and_recursive_exclusions() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::create_dir(root.join("data")).unwrap();
    std::fs::write(root.join("data/item"), "Acme").unwrap();
    let row = json!({"id":"project-remove","decision":"allow","conditions":{"domain":"argv","executable":"rm"},"requirements":{"arguments":{"kind":"options","prefix":[],"flags":["-f","-r","-R"],"values":[],"operands":true},"targets":{"action":"remove","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":".git"}]}}},"on_unmet":{"decision":"deny","message":"Preserve .git and remove only project paths."}});
    let evaluator = prepared(vec![row]);
    let mut req = request(&[]);
    req.project = root.to_string_lossy().into_owned();
    req.cwd = req.project.clone();
    req.executable = "rm".into();
    for (args, decision) in [
        (vec!["data/item"], Decision::Allow),
        (vec!["data/item", "../outside"], Decision::Deny),
        (vec!["-r", "."], Decision::Deny),
        (vec![".git"], Decision::Deny),
        (vec!["-r", "data"], Decision::Allow),
    ] {
        req.arguments = args.iter().map(|s| (*s).into()).collect();
        assert_eq!(evaluator.evaluate(&req).unwrap().decision, decision);
    }
    #[cfg(unix)]
    {
        let outside = assert_fs::TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
        req.arguments = vec!["escape/new".into()];
        assert_eq!(evaluator.evaluate(&req).unwrap().decision, Decision::Deny);
        req.arguments = vec!["escape".into()];
        assert_eq!(evaluator.evaluate(&req).unwrap().decision, Decision::Allow);
    }
}

#[test]
fn move_checks_effective_destination_and_overwrite_removals() {
    let dir = assert_fs::TempDir::new().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(project.join(".git")).unwrap();
    let ordinary = dir.path().join("acme.txt");
    std::fs::write(&ordinary, "Acme").unwrap();
    let within = json!({"within":{"root":"project"},"exclude":[{"root":"project","path":".git"}]});
    let row = json!({"id":"move","decision":"allow","conditions":{"domain":"argv","executable":"mv","arguments":{"kind":"any"}},"requirements":{"targets":[{"action":"remove","all":{"within":{"root":"absolute","path":dir.path().to_str().unwrap()},"exclude":[{"root":"project","path":".git"}]}},{"action":"write","all":within}]},"on_unmet":{"decision":"deny","message":"Move only into ordinary project paths."}});
    let evaluator = prepared(vec![row]);
    let mut request = Request {
        executable: "mv".into(),
        arguments: vec![ordinary.to_str().unwrap().into(), ".".into()],
        cwd: project.to_str().unwrap().into(),
        project: project.to_str().unwrap().into(),
    };
    assert_eq!(
        evaluator.evaluate(&request).unwrap().decision,
        Decision::Allow
    );
    request.arguments[1] = ".git".into();
    assert_eq!(
        evaluator.evaluate(&request).unwrap().decision,
        Decision::Deny
    );
    request.arguments[1] = "../outside".into();
    assert_eq!(
        evaluator.evaluate(&request).unwrap().decision,
        Decision::Deny
    );
    request.arguments = vec![
        "--unknown".into(),
        ordinary.to_str().unwrap().into(),
        ".".into(),
    ];
    assert_eq!(
        evaluator.evaluate(&request).unwrap().decision,
        Decision::Deny
    );
    // A source-only allow cannot establish coverage for the write destination.
    let partial = json!({"id":"partial","decision":"allow","conditions":{"domain":"argv","executable":"mv","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"remove","all":{"within":{"root":"absolute","path":dir.path().to_str().unwrap()}}}},"on_unmet":{"decision":"deny"}});
    request.arguments.remove(0);
    let value = json!(prepared(vec![partial]).evaluate(&request).unwrap());
    assert_eq!(value["decision"], "deny");
    assert!(
        value["evaluation"]["requirement_checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "evidence_unavailable")
    );
}

#[test]
#[cfg(unix)]
fn copy_follows_leaf_links_and_cached_locations_observe_each_request() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::write(root.join("source"), "Acme").unwrap();
    std::fs::write(outside.path().join("other"), "Other").unwrap();
    std::os::unix::fs::symlink(outside.path().join("other"), root.join("leaf")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("alias")).unwrap();
    let within = json!({"within":{"root":"project"}});
    let row = json!({"id":"copy","decision":"allow","conditions":{"domain":"argv","executable":"cp","arguments":{"kind":"any"}},"requirements":{"targets":[{"action":"read","all":within},{"action":"write","all":within}]},"on_unmet":{"decision":"deny"}});
    let evaluator = prepared(vec![row]);
    let restored =
        configuration::Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec())
            .unwrap();
    let mut request = Request {
        executable: "cp".into(),
        arguments: vec!["source".into(), "new".into()],
        cwd: root.to_str().unwrap().into(),
        project: root.to_str().unwrap().into(),
    };
    for candidate in [&evaluator, &restored] {
        assert_eq!(
            candidate.evaluate(&request).unwrap().decision,
            Decision::Allow
        );
        request.arguments[0] = "leaf".into();
        assert_eq!(
            candidate.evaluate(&request).unwrap().decision,
            Decision::Deny
        );
        request.arguments[0] = "source".into();
        request.arguments[1] = "alias/../escape".into();
        assert_eq!(
            candidate.evaluate(&request).unwrap().decision,
            Decision::Deny
        );
        request.arguments[1] = "new".into();
    }
    std::os::unix::fs::symlink(outside.path().join("other"), root.join("new")).unwrap();
    assert_eq!(
        restored.evaluate(&request).unwrap().decision,
        Decision::Deny
    );
}

#[test]
fn shell_file_redirections_and_nested_requirements_reach_agent_diagnostics() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    std::fs::create_dir(format!("{root}/.git")).unwrap();
    let file = json!({"id":"writes","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"project"},"exclude":[{"root":"project","path":".git"}]}},"on_unmet":{"decision":"deny","message":"Preserve .git; choose an ordinary project file."}});
    let readonly = json!({"id":"mysql","decision":"allow","conditions":{"domain":"argv","executable":"mysql"},"requirements":{"arguments":{"kind":"options","prefix":[],"flags":["--read-only"],"values":[],"operands":false,"require_all":["--read-only"]}},"on_unmet":{"decision":"deny","message":"Add --read-only and submit again.","alternatives":["mysql --read-only"]}});
    let allow = json!({"id":"echo","decision":"allow","conditions":{"domain":"argv","executable":"echo","arguments":{"kind":"any"}}});
    let evaluator = prepared(vec![file, readonly, allow]);
    let context = Context {
        cwd: root.clone(),
        environment: None,
        working_files: None,
    };
    for (source, decision, message) in [
        ("echo hi > report.txt", "allow", None),
        ("echo hi > .git/config", "deny", Some("Preserve .git")),
        ("mysql", "deny", Some("Add --read-only")),
        ("mysql --read-only", "allow", None),
    ] {
        let input = Input::Shell {
            source: source.into(),
            dialect: "posix".into(),
            dialect_version: None,
            original_input: json!({"command":source}),
        };
        let value = evaluator
            .evaluate_normalized(
                &input,
                &context,
                &root,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(value["decision"], decision, "{source}");
        if let Some(message) = message {
            let details = crate::agent_message::detail(&value["explanation"]).unwrap();
            assert!(details.contains(message));
            assert!(details.contains("Re-evaluate"));
            assert!(
                value["explanation"]["requirement_checks"][0]["operation_ref"]
                    .as_str()
                    .unwrap()
                    .starts_with("shell:")
            );
        }
    }
}

#[test]
fn shell_explanations_carry_the_rules_that_decided_their_commands() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let evaluator = prepared(vec![
        json!({"id":"text","decision":"allow","reason":"Text output is routine.","conditions":{"domain":"argv","executable":"echo","arguments":{"kind":"any"}}}),
        json!({"id":"acme-config","decision":"deny","reason":"Configuration changes need the user.","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["config"]}}}),
    ]);
    let context = Context {
        cwd: root.clone(),
        environment: None,
        working_files: None,
    };
    for (source, decision, rule) in [
        ("echo hi", "allow", "text"),
        ("echo hi && acme config set x", "deny", "acme-config"),
    ] {
        let input = Input::Shell {
            source: source.into(),
            dialect: "bash".into(),
            dialect_version: None,
            original_input: json!({"command":source}),
        };
        let value = evaluator
            .evaluate_normalized(
                &input,
                &context,
                &root,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )
            .unwrap();
        let explanation = &value["explanation"];
        assert_eq!(value["decision"], decision, "{source}");
        assert_eq!(explanation["reason"], "matched-policy", "{source}");
        let matches = explanation["matches"].as_array().unwrap();
        assert!(
            matches.iter().any(|m| m["rule_id"] == rule
                && m["applied"] == true
                && m["operation_ref"]
                    .as_str()
                    .is_some_and(|r| r.starts_with("shell:"))),
            "{source}: {explanation}"
        );
        // Only commands with the final decision explain it.
        assert!(
            matches.iter().all(|m| m["decision"] == decision),
            "{source}: {explanation}"
        );
    }
    let value = evaluator
        .evaluate_normalized(
            &Input::Shell {
                source: "acme config set x".into(),
                dialect: "bash".into(),
                dialect_version: None,
                original_input: json!({"command":"acme config set x"}),
            },
            &context,
            &root,
            None,
            &crate::evidence::now(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(
        crate::agent_message::detail(&value["explanation"]).as_deref(),
        Some(
            "Rule `acme-config` denies this request: Configuration changes need the user. Re-evaluate any revised request."
        )
    );
}

#[test]
fn every_literal_command_that_asks_in_a_chain_keeps_its_own_guidance() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let evaluator = prepared(vec![
        json!({"id":"text","decision":"allow","conditions":{"domain":"argv","executable":"echo","arguments":{"kind":"any"}}}),
    ]);
    let context = Context {
        cwd: root.clone(),
        environment: None,
        working_files: None,
    };
    let source = "acme-format && echo hi && acme-lint --fix && acme-format";
    let value = evaluator
        .evaluate_normalized(
            &Input::Shell {
                source: source.into(),
                dialect: "bash".into(),
                dialect_version: None,
                original_input: json!({"command":source}),
            },
            &context,
            &root,
            None,
            &crate::evidence::now(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(value["decision"], "ask");
    let guidance = value["explanation"]["guidance"].as_array().unwrap();
    // Feedback leads with the first; each asked command can get a rule.
    assert!(
        guidance[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("`acme-format`")
    );
    let commands: Vec<_> = guidance
        .iter()
        .filter_map(crate::proposal_draft::decisive_command)
        .map(|change| change["rule"]["conditions"]["executable"].clone())
        .collect();
    assert_eq!(
        commands,
        [
            json!("acme-format"),
            json!("acme-lint"),
            json!("acme-format")
        ]
    );
}

#[test]
fn uninspectable_shell_parts_explain_their_ask_to_agents() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let evaluator = prepared(vec![
        json!({"id":"text","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["echo","cat","find","node"]},"arguments":{"kind":"any"}}}),
        json!({"id":"review","decision":"ask","conditions":{"domain":"argv","executable":"acme-review","arguments":{"kind":"any"}},"guidance":{"alternatives":["acme-review --dry-run"]}}),
    ]);
    let context = Context {
        cwd: root.clone(),
        environment: None,
        working_files: None,
    };
    for (source, decision, guidance) in [
        ("echo hi | cat", "allow", None),
        (
            "cat \"$(acme-unlisted)\"",
            "ask",
            Some("`cat \"$(acme-unlisted)\"` has words or control flow"),
        ),
        ("eval \"echo hi\"", "ask", Some("`eval \"echo hi\"`")),
        // An exported or zsh-tied name would reach later commands.
        (
            "F=acme.txt; cat $F",
            "ask",
            Some("Write the value directly"),
        ),
        (
            "NODE_ENV=test cat",
            "ask",
            Some("`NODE_ENV=test cat` sets environment variables"),
        ),
        // A value known only when it runs cannot be written in instead.
        (
            "cat & pid=$!; cat",
            "ask",
            Some("`pid=$!` sets a shell variable from a value known only when it runs"),
        ),
        ("find . -name '*.txt' -type f | cat", "allow", None),
        // Rewriting cannot make launched programs inspectable; the user decides.
        (
            "find . -exec cat {} \\;",
            "ask",
            Some("runs a program whose effects Shield cannot inspect, so the user decides"),
        ),
        (
            "node acme.js",
            "ask",
            Some("`node acme.js` runs a program whose effects Shield cannot inspect"),
        ),
        ("node -v", "allow", None),
        ("node --version && cat", "allow", None),
        (
            "acme-review",
            "ask",
            Some(
                "`acme-review` asks for approval under rule review. Candidate: acme-review --dry-run",
            ),
        ),
        // In a long command, the message names the one part that asks.
        (
            "cat acme.txt && echo hi > acme.txt && acme-review && echo done",
            "ask",
            Some("`acme-review` asks for approval"),
        ),
        (
            "acme-unlisted",
            "ask",
            Some("`acme-unlisted` matches no rule"),
        ),
    ] {
        let input = Input::Shell {
            source: source.into(),
            dialect: "bash".into(),
            dialect_version: None,
            original_input: json!({"command":source}),
        };
        let value = evaluator
            .evaluate_normalized(
                &input,
                &context,
                &root,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(value["decision"], decision, "{source}: {value}");
        let detail = crate::agent_message::detail(&value["explanation"]);
        match guidance {
            Some(text) => assert!(
                detail.as_ref().unwrap().contains(text),
                "{source}: {detail:?}"
            ),
            None => assert!(detail.is_none(), "{source}: {detail:?}"),
        }
    }
}

#[test]
fn option_parsing_controls_keep_arity_and_repetition_authoritative() {
    let options = json!({"kind":"options","prefix":[],"flags":["-a","-b"],"values":[{"name":"--format","match":"json","repeat":"last"}],"optional_values":["--color"],"operands":false,"short_clusters":false,"long_equals":false,"short_attached":false,"require_all":["--format"],"forbid":["-b"]});
    for (arguments, allowed) in [
        (vec!["--format", "text", "--format", "json"], true),
        (vec!["--format", "json", "--format", "text"], false),
        (vec!["--format=json"], false),
        (vec!["-ab", "--format", "json"], false),
        (vec!["-b", "--format", "json"], false),
    ] {
        assert_eq!(
            evaluate(rule(options.clone()), &arguments)["decision"],
            if allowed { "allow" } else { "ask" }
        );
    }
}

#[test]
fn skipped_options_find_declared_global_options_anywhere() {
    // An argparse-style tool reads its global options anywhere and accepts any
    // unique prefix of their long names.
    let scan = json!({"kind":"options","prefix":[],"flags":["--debug","--no-paginate"],
        "values":[{"name":"--profile","match":"acme-ro"},"--region","--output"],
        "operands":true,"unknown_options":"skip","long_abbreviations":true,
        "require_all":["--profile"]});
    for (args, allowed) in [
        (
            vec![
                "logs",
                "filter-log-events",
                "--log-group-name",
                "g",
                "--profile",
                "acme-ro",
            ],
            true,
        ),
        (
            vec!["--profile=acme-ro", "s3", "ls", "-x", "--page-size", "5"],
            true,
        ),
        (vec!["s3", "ls", "--prof", "acme-ro"], true),
        (vec!["s3", "ls", "--p=acme-ro"], true),
        (vec!["s3", "ls", "--prof", "acme-admin"], false),
        (
            vec!["s3", "ls", "--profile", "acme-ro", "--pr=acme-admin"],
            false,
        ),
        (vec!["s3", "ls", "--filter", "x"], false),
        // "--profile" after a separator is an operand, so the option is missing.
        (vec!["s3", "ls", "--", "--profile", "acme-ro"], false),
        // An undeclared option cannot hide the next token from the scan.
        (vec!["s3", "ls", "--query", "--profile", "acme-ro"], true),
    ] {
        assert_eq!(
            evaluate(rule(scan.clone()), &args)["decision"],
            if allowed { "allow" } else { "ask" },
            "{args:?}"
        );
    }
    // A prefix of one declared name selects it; a prefix of several is
    // refused rather than guessed.
    let unique = argument_rules::matches(
        &scan,
        &["--p", "acme-ro"].map(String::from),
        &crate::policy::patterns::Store::default(),
    )
    .unwrap();
    assert_eq!(unique.failures, [] as [&str; 0]);
    assert_eq!(unique.records[0].name, "--profile");
    let mut both = scan.clone();
    both["values"] = json!(["--profile", "--prefix"]);
    let parsed = argument_rules::matches(
        &both,
        &["--pr", "acme-ro"].map(String::from),
        &crate::policy::patterns::Store::default(),
    )
    .unwrap();
    assert_eq!(
        parsed.failures,
        ["ambiguous_option", "required_option_missing"]
    );
    // Without the setting, abbreviations stay unknown options.
    let mut exact = scan.clone();
    exact.as_object_mut().unwrap().remove("long_abbreviations");
    exact["unknown_options"] = json!("refuse");
    exact["operands"] = json!(true);
    assert_eq!(
        evaluate(rule(exact), &["--prof", "acme-ro"])["decision"],
        "ask"
    );
    // Skipping needs operands: true and intermixed parsing.
    for invalid in [
        json!({"kind":"options","prefix":[],"flags":[],"values":[],"operands":false,"unknown_options":"skip"}),
        json!({"kind":"options","prefix":[],"flags":[],"values":[],"operands":true,"unknown_options":"skip","parse":"stop_at_operand"}),
        json!({"kind":"options","prefix":[],"flags":[],"values":[],"operands":true,"unknown_options":"ignore"}),
        json!({"kind":"prefix","values":["x"],"leading":{"flags":[],"values":[],"unknown_options":"skip"}}),
    ] {
        assert!(argument_rules::validate(&invalid).is_err(), "{invalid}");
    }
    let source = document(&[rule(scan)]).unwrap();
    assert_eq!(
        source["capabilities"],
        json!([
            "argument-rules-v1",
            "option-abbreviations-v1",
            "structured-rules-v1",
            "unknown-options-v1"
        ])
    );
}

#[test]
fn skipped_options_refuse_long_prefixes_when_abbreviations_are_off() {
    let scan = json!({"kind":"options","prefix":[],"flags":["--debug"],
        "values":[{"name":"--profile","match":{"not":"acme-prod"},"repeat":"last"},"--prefix"],
        "operands":true,"unknown_options":"skip","require_all":["--profile"]});
    for explicit_false in [false, true] {
        let mut grammar = scan.clone();
        if explicit_false {
            grammar["long_abbreviations"] = json!(false);
        }
        let row = json!({"id":"profile","decision":"allow",
            "conditions":{"domain":"argv","executable":"acme"},
            "requirements":{"arguments":grammar},"on_unmet":{"decision":"deny"}});
        for (suffix, allowed) in [
            (vec![], true),
            (vec!["--unrelated", "value"], true),
            (vec!["--profile-extra=acme-prod"], true),
            (vec!["--prof", "acme-prod"], false),
            (vec!["--prof=acme-prod"], false),
            (vec!["--pr=acme-prod"], false),
            (vec!["--d"], false),
            (vec!["--", "--prof", "acme-prod"], true),
        ] {
            let mut args = vec!["s3", "ls", "--profile", "acme-dev"];
            args.extend(suffix);
            let result = evaluate(row.clone(), &args);
            assert_eq!(
                result["decision"],
                if allowed { "allow" } else { "deny" },
                "explicit_false={explicit_false}, {args:?}"
            );
        }
    }
}

#[test]
fn skipped_options_refuse_short_clusters_that_hide_declared_options() {
    let grammar = json!({"kind":"options","prefix":[],"flags":["-v"],
        "values":[{"name":"-X","match":"GET","repeat":"last"},"-o"],
        "operands":true,"unknown_options":"skip","require_all":["-X"],"forbid":["-v"]});
    let row = json!({"id":"method","decision":"allow",
        "conditions":{"domain":"argv","executable":"acme"},
        "requirements":{"arguments":grammar},"on_unmet":{"decision":"deny"}});
    let image = prepared(vec![row.clone()]);
    let restored =
        configuration::Evaluator::from_authenticated_compiled(image.compiled_bytes().to_vec())
            .unwrap();
    for (suffix, allowed) in [
        (vec![], true),
        (vec!["-k"], true),
        (vec!["-kzz", "value"], true),
        (vec!["--unrelated=XPOST"], true),
        (vec!["-oXPOST"], true),
        (vec!["--", "-kXPOST"], true),
        (vec!["-kXPOST"], false),
        (vec!["-kX", "POST"], false),
        (vec!["-kkXPOST"], false),
        (vec!["-kv"], false),
        (vec!["-XPOST"], false),
    ] {
        let mut args = vec!["-X", "GET"];
        args.extend(suffix);
        let expected = if allowed {
            Decision::Allow
        } else {
            Decision::Deny
        };
        assert_eq!(
            evaluate(row.clone(), &args)["decision"],
            json!(expected),
            "{args:?}"
        );
        for image in [&image, &restored] {
            assert_eq!(
                image.evaluate(&request(&args)).unwrap().decision,
                expected,
                "{args:?}"
            );
        }
    }
}

#[test]
fn negated_selectors_match_present_values_outside_their_inner_selector() {
    let blocked = json!({"kind":"options","prefix":[],"flags":[],
        "values":[{"name":"--profile","match":{"not":{"one_of":["acme-prod","acme-admin"]}}}],
        "operands":true,"unknown_options":"skip","require_all":["--profile"]});
    assert_eq!(
        evaluate(rule(blocked.clone()), &["s3", "ls", "--profile", "acme-ro"])["decision"],
        "allow"
    );
    for args in [
        vec!["s3", "ls", "--profile", "acme-prod"],
        vec![
            "s3",
            "ls",
            "--profile",
            "acme-ro",
            "--profile",
            "acme-admin",
        ],
        vec!["s3", "ls"],
    ] {
        assert_eq!(
            evaluate(rule(blocked.clone()), &args)["decision"],
            "ask",
            "{args:?}"
        );
    }
    let row = json!({"id":"blocked","decision":"allow","conditions":{"domain":"argv","executable":"acme"},
        "requirements":{"arguments":blocked},"on_unmet":{"decision":"deny"}});
    let message =
        |args: &[&str]| crate::agent_message::detail(&evaluate(row.clone(), args)).unwrap();
    assert!(
        message(&["--profile", "acme-prod"])
            .contains("--profile must not be any of acme-prod, acme-admin.")
    );
    let mut one = row.clone();
    one["requirements"]["arguments"]["values"][0]["match"] = json!({"not":"acme-prod"});
    assert!(
        crate::agent_message::detail(&evaluate(one, &["--profile", "acme-prod"]))
            .unwrap()
            .contains("--profile must not be acme-prod.")
    );
    // Patterns inside a negation are prepared and declared like any other.
    let pattern = rule(json!({"kind":"prefix","values":[{"not":{"regex":"prod-.*"}}]}));
    assert_eq!(evaluate(pattern.clone(), &["dev-1"])["decision"], "allow");
    assert_eq!(evaluate(pattern.clone(), &["prod-1"])["decision"], "ask");
    assert_eq!(
        document(&[pattern]).unwrap()["capabilities"],
        json!([
            "argument-rules-v1",
            "negated-selectors-v1",
            "regex-v1",
            "structured-rules-v1"
        ])
    );
    for invalid in [
        json!({"not":{"not":"x"}}),
        json!({"not":"x","exact":"y"}),
        json!({"not":[]}),
    ] {
        assert!(selectors::text(&invalid, false).is_err(), "{invalid}");
    }
}

#[test]
fn generated_requirement_feedback_names_missing_options_without_values() {
    let row = json!({"id":"required","decision":"allow","conditions":{"domain":"argv","executable":"acme"},"requirements":{"arguments":{"kind":"options","prefix":[],"flags":["--read-only"],"values":["--password"],"operands":false,"require_all":["--read-only"]}},"on_unmet":{"decision":"deny"}});
    let value = evaluate(row, &["--password", "Acme-private-value"]);
    assert_eq!(value["decision"], "deny");
    assert!(!value.to_string().contains("Acme-private-value"));
    assert!(
        crate::agent_message::detail(&value)
            .unwrap()
            .contains("--read-only")
    );
    let row = json!({"id":"literal","decision":"allow","conditions":{"domain":"mcp","server":"acme","tool":"inspect","fields":{"/metadata":{"exact":{"root":"project","regex":"literal"}}}}});
    let source = document(&[row]).unwrap();
    assert_eq!(source["capabilities"], json!(["structured-rules-v1"]));
    assert!(CustomRules::from_yaml(&source.to_string()).is_ok());
}

#[test]
fn feedback_offers_the_alternative_in_the_form_the_request_used() {
    let row = json!({"id":"search","decision":"allow","conditions":{"domain":"argv","executable":"acme"},"requirements":{"arguments":{"kind":"options","prefix":[],"flags":["--no-config","--files","-n"],"values":[],"operands":true,"require_all":["--no-config"]}},"on_unmet":{"decision":"deny","alternatives":["acme --no-config -n PATTERN","acme --no-config --files"]}});
    for (arguments, expected) in [
        (&["--files"][..], "Candidate: acme --no-config --files."),
        (
            &["-n", "acme"][..],
            "Candidate: acme --no-config -n PATTERN.",
        ),
        (&["acme"][..], "Candidate: acme --no-config -n PATTERN."),
    ] {
        let value = evaluate(row.clone(), arguments);
        assert_eq!(value["decision"], "deny");
        let detail = crate::agent_message::detail(&value).unwrap();
        assert!(detail.contains(expected), "{arguments:?}: {detail}");
    }
}

#[test]
fn alternatives_only_guidance_is_delivered_without_inventing_a_failure() {
    let row = json!({"id":"candidate","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"guidance":{"alternatives":["acme inspect --read-only"]}});
    let value = evaluate(row, &[]);
    assert_eq!(value["decision"], "allow");
    assert_eq!(value["requirement_checks"], json!([]));
    assert_eq!(
        crate::agent_message::detail(&value).as_deref(),
        Some("Candidate: acme inspect --read-only. Re-evaluate any revised request.")
    );
    assert!(crate::agent_message::detail(&json!({})).is_none());
}

#[test]
fn cache_repair_command_preserves_the_project_as_one_literal_argument() {
    let project = "/workspace/acme's $(echo private); project";
    let route = crate::agent_message::cache_repair(project).unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let evaluator =
        configuration::Evaluator::new(vec![(Scope::Project, bundle.snapshot().unwrap())]).unwrap();
    let parsed = crate::shell::analyze(
        &route,
        crate::shell::Dialect::Posix,
        "/workspace",
        "/workspace",
        &evaluator,
    )
    .unwrap();
    let operations = parsed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1, "{parsed}");
    assert_eq!(
        operations[0]["request"]["arguments"],
        json!(["cache", "prepare", "--project", project])
    );
    for invalid in [
        "relative",
        "/workspace/acme\nproject",
        "/workspace/acme\0project",
    ] {
        assert!(crate::agent_message::cache_repair(invalid).is_none());
    }
}

#[test]
#[cfg(unix)]
fn ripgrep_accepts_ordinary_searches_and_checks_visible_options_and_paths() {
    let dir = assert_fs::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.js"), "Acme").unwrap();
    let evaluator = prepared(vec![
        json!({"id":"tree","decision":"allow","conditions":{"domain":"argv","executable":"rg","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":".aws"}]}}},"on_unmet":{"decision":"deny"}}),
    ]);
    let decide = |args: &[&str]| {
        let req = Request {
            executable: "rg".into(),
            arguments: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: root.to_string_lossy().into_owned(),
            project: root.to_string_lossy().into_owned(),
        };
        evaluator.evaluate(&req).unwrap().decision
    };
    for args in [
        &["Acme"][..],
        &["--files"],
        &["-n", "Acme", "src/app.js"],
        &["-g", "*.js", "Acme", "src"],
        &["--files", "-g", "*.js"],
        &["--files", "--hidden", "-g", "!.git", "."],
        &["--hidden", "--files"],
        &["--no-ignore", "--hidden", "--files"],
        &["--no-config", "--no-ignore", "Acme"][..],
        &[
            "--no-config",
            "--no-ignore",
            "-n",
            "-S",
            "-C",
            "2",
            "-tjs",
            "Acme",
            "src",
        ],
        &["--no-config", "-g", "*.js", "-m3", "Acme", "src/app.js"],
        &["--no-config", "--files"],
        &["--no-config", "--files", "src"],
        &["--no-config", "-l", "--", "-Acme", "src/app.js"],
        &["--no-config", "Acme"],
        &["--no-config", "Acme", "src"],
        &["--no-config", "-g", "*.js", "Acme", "src"],
        &["--no-config", "--files", "-g", "*.js"],
        &["--no-config", "--no-ignore", "-g", "*.js", "Acme", "src"],
        &["--no-config", "--no-ignore", "--glob=*.js", "Acme"],
    ] {
        assert_eq!(decide(args), Decision::Allow, "{args:?}");
    }
    for args in [
        &["--pre", "sh", "Acme"][..],
        &["--follow", "Acme", "src"],
        &["--hidden", "Acme", "src"],
        &["--no-config", "Acme", "src", "-n"],
        &["--no-config", "--pre", "sh", "Acme"],
        &["--no-config", "--hidden", "Acme"],
        &["--no-config", "-g"],
        &["--no-config", "Acme", "missing"],
    ] {
        assert_eq!(decide(args), Decision::Deny, "{args:?}");
    }
    // A listing without --no-ignore counts every existing entry, so an existing
    // credential directory blocks it; the visible content walk skips it.
    std::fs::create_dir(root.join(".aws")).unwrap();
    assert_eq!(decide(&["Acme", ".aws"]), Decision::Deny);
    assert_eq!(decide(&["Acme"]), Decision::Deny);
    assert_eq!(decide(&["--no-config", "--files"]), Decision::Deny);
    assert_eq!(decide(&["--files", "--hidden"]), Decision::Deny);
    assert_eq!(
        decide(&["--no-ignore", "--files", "--hidden"]),
        Decision::Deny
    );
    assert_eq!(
        decide(&["--no-config", "--no-ignore", "Acme", "."]),
        Decision::Allow
    );
    assert_eq!(
        decide(&["--no-config", "Acme", "src/app.js"]),
        Decision::Allow
    );
}

#[test]
#[cfg(unix)]
fn fixed_text_filters_bind_each_file_and_reject_effectful_programs() {
    let root = assert_fs::TempDir::new().unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::write(root.path().join("item"), "Acme").unwrap();
    std::fs::write(outside.path().join("secret"), "Acme").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("escape")).unwrap();
    std::fs::create_dir(root.path().join(".aws")).unwrap();
    std::fs::write(root.path().join(".aws/config"), "Acme").unwrap();
    let evaluator = prepared(vec![
        json!({"id":"read","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["sed","awk","jq","sort","uniq","cut","diff","cmp"]},"arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[{"root":"project","path":".aws"}]}}},"on_unmet":{"decision":"deny"}}),
    ]);
    for (executable, arguments, expected) in [
        ("sed", vec!["-n", "1,20p", "item"], Decision::Allow),
        (
            "awk",
            vec!["-F:", "{print $1, $2}", "item"],
            Decision::Allow,
        ),
        (
            "jq",
            vec!["-r", ".items[0].name | tostring", "item"],
            Decision::Allow,
        ),
        ("sort", vec!["-k1,1", "item"], Decision::Allow),
        ("uniq", vec!["-c", "item"], Decision::Allow),
        ("cut", vec!["-d:", "-f1", "item"], Decision::Allow),
        ("diff", vec!["-U3", "item", "item"], Decision::Allow),
        ("cmp", vec!["-s", "item", "item"], Decision::Allow),
        ("sed", vec!["-i", "1,20p", "item"], Decision::Deny),
        ("sed", vec!["-n", "1p; e id", "item"], Decision::Deny),
        (
            "awk",
            vec!["{print $1 > \"output\"}", "item"],
            Decision::Deny,
        ),
        ("jq", vec!["env", "item"], Decision::Deny),
        ("jq", vec!["-f", "item", "item"], Decision::Deny),
        ("sort", vec!["-o", "item", "item"], Decision::Deny),
        ("uniq", vec!["item", "output"], Decision::Deny),
        ("sed", vec!["p", "escape"], Decision::Deny),
        ("awk", vec!["{print $1}", ".aws/config"], Decision::Deny),
        ("jq", vec![".", "missing"], Decision::Deny),
        ("diff", vec!["-u", "item", "escape"], Decision::Deny),
    ] {
        let request = Request {
            executable: executable.into(),
            arguments: arguments.iter().map(|s| (*s).into()).collect(),
            cwd: root.path().to_string_lossy().into_owned(),
            project: root.path().to_string_lossy().into_owned(),
        };
        assert_eq!(
            evaluator.evaluate(&request).unwrap().decision,
            expected,
            "{request:?}"
        );
    }
}
