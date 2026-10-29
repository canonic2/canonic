//! Rule scope, requirement transitions and honest summary bounds.
use super::*;
use serde_json::json;

#[test]
fn confirmation_names_exact_commands_before_rule_metadata() {
    for (decision, expected) in [
        ("allow", "Allow `npm test` without asking."),
        ("ask", "Ask before running `npm test`."),
        ("deny", "Block `npm test`."),
    ] {
        let lines = rule_lines(
            &json!({"change":"added","rule":{
                "id":"agent-chosen-name","decision":decision,"conditions":{
                    "domain":"argv","executable":"npm",
                    "arguments":{"kind":"exact","values":["test"]}
                }
            }}),
            "",
        );
        assert_eq!(lines[0], expected);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("no additional arguments"))
        );
        assert_eq!(lines.last().unwrap(), "Rule ID: agent-chosen-name (added).");
    }
    let lines = rule_lines(
        &json!({"change":"added","rule":{
            "id":"example","decision":"allow","conditions":{
                "domain":"argv","executable":"acme",
                "arguments":{"kind":"exact","values":["two words", ""]}
            }
        }}),
        "",
    );
    assert_eq!(lines[0], "Allow `acme 'two words' ''` without asking.");
}

#[test]
fn confirmation_marks_exact_arguments_omitted_from_the_headline() {
    for count in [8, 10] {
        let values: Vec<_> = (0..count).map(|i| format!("arg{i}")).collect();
        let lines = rule_lines(
            &json!({"change":"added","rule":{
                "id":"long-command","decision":"allow","conditions":{
                    "domain":"argv","executable":"acme",
                    "arguments":{"kind":"exact","values":values}
                }
            }}),
            "",
        );
        assert!(lines[0].contains("arg7"));
        assert_eq!(lines[0].contains('…'), count > 8);
        assert_eq!(
            lines
                .iter()
                .any(|line| line.contains("not a complete scope description")),
            count > 8
        );
        if count > 8 {
            assert!(lines[0].ends_with("arg7 …` without asking."));
        }
    }
}

#[test]
fn confirmation_shows_path_roots_exclusions_patterns_and_quantifiers() {
    let scope = json!({"cwd":{"within":{"root":"project","path":"src"}},
        "sources":{"all":{"within":{"root":"read"},"exclude":[{"root":"absolute","path":"/srv/private"}],"match":{"glob":"*.txt"}}},
        "destination":{"exact":{"root":"write","path":"reports/out.txt"}},
        "targets":[{"action":"read","all":{"within":{"root":"read"}}},
            {"action":"write","any":{"outside":{"root":"project"}}}]});
    let description = predicates(&scope, "file", false);
    for expected in [
        "working directory: within project directory / src",
        "source paths: all within approved read folders",
        "excluding /srv/private",
        "path matching glob: *.txt",
        "destination: exactly approved write folders / reports/out.txt",
        "read: all within approved read folders",
        "write: at least one outside project directory",
    ] {
        assert!(description.contains(expected), "{description}");
    }
    assert!(!description.contains('{'));
}

#[test]
fn confirmation_shows_conditions_requirements_and_unmet_changes_separately() {
    let before = json!({"id":"fetch", "decision":"allow",
        "conditions":{"domain":"web","action":"fetch","method":"GET", "url":{"schemes":["https"],"hosts":["docs.example.com"],"ports":[443],"path":{"prefix":"/guide"}}},
        "requirements":{"url":{"schemes":["https"],"hosts":["docs.example.com"],"path":{"exact":"/guide/start"}}},
        "on_unmet":{"decision":"deny"}});
    let mut after = before.clone();
    after["requirements"]["url"]["path"] = json!({"prefix":"/guide"});
    after["on_unmet"]["decision"] = json!("ask");
    let message = rule_lines(
        &json!({"kind":"rule","change":"edited","before":before,"rule":after}),
        "",
    )
    .join("\n");
    let (before, after) = message.split_once("After: ").unwrap();
    assert!(before.contains("Before: allow for web fetch requests"));
    assert!(before.contains("HTTP method: GET"));
    assert!(before.contains("ports: 443"));
    assert!(before.contains("Requirements: URL schemes: https; hosts: docs.example.com; ports: any; URL path: exact: /guide/start"));
    assert!(before.contains("If requirements fail or cannot be verified: deny"));
    assert!(after.contains("URL path: prefix: /guide"));
    assert!(after.contains("If requirements fail or cannot be verified: ask"));
    assert!(!message.contains('{'));
}

#[test]
fn confirmation_does_not_silently_hide_unlisted_or_truncated_scope() {
    let hosts: Vec<_> = (0..10).map(|i| format!("docs{i}.example.com")).collect();
    let message = rule_lines(&json!({"kind":"rule","change":"added","rule":{
        "id":"fetch","decision":"allow","conditions":{"domain":"web","action":"fetch"},
        "requirements":{"url":{"schemes":["https"],"hosts":hosts}},"on_unmet":{"decision":"deny"}
    }}), "").join("\n");
    assert!(message.contains("2 more entries not shown"));
    assert!(message.contains("not a complete scope description"));
    let message = rule_lines(&json!({"kind":"rule","change":"added","rule":{
        "id":"file","decision":"allow","conditions":{"domain":"file","action":"read"},
        "requirements":{"sources":{"all":{"within":{"root":"absolute","path":format!("/srv/{}", "folder".repeat(80))}}}},"on_unmet":{"decision":"deny"}
    }}), "").join("\n");
    assert!(message.contains('…'));
    assert!(message.contains("not a complete scope description"));
}

#[test]
fn confirmation_keeps_required_arguments_and_mcp_fields_visible() {
    assert!(predicates(&json!({"arguments":{"kind":"options","prefix":["inspect"],"flags":["--safe"],"values":[],"operands":false,"require_all":["--safe"]}}), "argv", false).contains("all required options: --safe"));
    let fields = predicates(
        &json!({"arguments":{"kind":{"exact":"preview"}}}),
        "mcp",
        true,
    );
    assert!(fields.contains("request field constraints: kind: exact: preview"));
    let paths = predicates(
        &json!({"argument_paths":{
            "grammar":{"kind":"options","prefix":["read"],"flags":[],"values":["--input"],"operands":false},
            "options":{"--input":{"path":{"within":{"root":"project","path":"src"}},"recursive":false}}
        }}),
        "argv",
        false,
    );
    assert!(
        paths.contains("option --input paths: within project directory / src; recursive: disabled")
    );
}
