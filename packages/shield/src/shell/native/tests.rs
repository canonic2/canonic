//! Dedicated native grammars preserve argv/spans and refuse an unsafe suffix.
use super::*;
use crate::shell::{cmd, powershell};

fn commands(node: &Node) -> Vec<&[Word]> {
    match node {
        Node::Command { words, .. } => vec![words],
        Node::Sequence { children } => children.iter().flat_map(commands).collect(),
        Node::Binary { left, right, .. } => {
            commands(left).into_iter().chain(commands(right)).collect()
        }
        Node::Decorated { body, .. } => commands(body),
        _ => panic!("not a literal native node"),
    }
}

fn arguments(node: &Node) -> Vec<Vec<&str>> {
    commands(node)
        .into_iter()
        .map(|words| {
            words
                .iter()
                .map(|word| word.literal.as_deref().unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn powershell_sequences_and_literal_call_operator_preserve_native_words() {
    let source = "git -C 'C:\\Acme Tools\\project' status; rg 'don''t' \"notes here.txt\"\r\n& 'node' --version";
    let node = powershell::parse(source, &|| Ok(())).unwrap().unwrap();
    assert_eq!(
        arguments(&node),
        [
            vec!["git", "-C", "C:\\Acme Tools\\project", "status"],
            vec!["rg", "don't", "notes here.txt"],
            vec!["node", "--version"]
        ]
    );
    for word in commands(&node).into_iter().flatten() {
        assert_eq!(&source[word.span.start..word.span.end], word.raw);
    }
}

#[test]
fn cmd_sequences_chains_and_single_quotes_keep_cmd_semantics() {
    let source = "git -C \"C:\\Acme Tools\\project\" status & rg 'two words' file && node --version || shield --version";
    let node = cmd::parse(source, &|| Ok(())).unwrap().unwrap();
    assert_eq!(
        arguments(&node),
        [
            vec!["git", "-C", "C:\\Acme Tools\\project", "status"],
            vec!["rg", "'two", "words'", "file"],
            vec!["node", "--version"],
            vec!["shield", "--version"]
        ]
    );
    for word in commands(&node).into_iter().flatten() {
        assert_eq!(&source[word.span.start..word.span.end], word.raw);
    }
    let powershell = powershell::parse("rg 'two words' file", &|| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(arguments(&powershell), [vec!["rg", "two words", "file"]]);
}

#[test]
fn unknown_native_syntax_never_exposes_only_the_safe_prefix() {
    for source in [
        "git status; Remove-Item .shield",
        "git status; git $(Get-Content notes.txt)",
        "git status && git diff",
        "git status | rg x",
        "git status > out.txt",
        "git $HOME",
        "git @args",
        "git --% $HOME",
        "git ''",
        "git 'a'joined",
        "git \"a`\"b\"",
        "git 'a\"b'",
        "git \u{201c}status\u{201d}",
        "git status & node --version",
    ] {
        assert!(
            powershell::parse(source, &|| Ok(())).unwrap().is_none(),
            "{source}"
        );
    }
    for source in [
        "git %NAME%",
        "git \"!NAME!\"",
        "git ^& echo x",
        "git status & call git diff",
        "git status | rg x",
        "git status > out.txt",
        "(git status)",
        "git \"unterminated",
        "@git status",
        "git.exe status",
        "git \"C:\\Acme\\\"",
        "& git status",
        "git status &",
        "git status & & git diff",
        "git status && & git diff",
    ] {
        assert!(
            cmd::parse(source, &|| Ok(())).unwrap().is_none(),
            "{source}"
        );
    }
    assert!(matches!(
        powershell::parse("git 'unterminated", &|| Ok(())),
        Err(Error::Contract(_))
    ));
}

#[test]
fn native_argument_token_and_chain_budgets_check_cancellation() {
    for parse in [powershell::parse, cmd::parse] {
        assert!(matches!(
            parse(&format!("git {}", "arg ".repeat(257)), &|| Ok(())),
            Err(Error::Limit)
        ));
        assert!(matches!(
            parse("git status", &|| Err(Error::Interrupted)),
            Err(Error::Interrupted)
        ));
    }
    assert!(matches!(
        cmd::parse(&vec!["git status"; 34].join(" && "), &|| Ok(())),
        Err(Error::Limit)
    ));
}

fn evaluator() -> crate::configuration::Evaluator {
    let rules = crate::policy::rules::document(&[
        serde_json::json!({"id":"status","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}}),
        serde_json::json!({"id":"push","decision":"deny","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"prefix","values":["push"]}}}),
        serde_json::json!({"id":"node","decision":"allow","conditions":{"domain":"argv","executable":"node","arguments":{"kind":"any"}}}),
    ]).unwrap().to_string();
    let snapshot = crate::configuration::Candidate::from_yaml(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap()
    .capture(&[], Some(&rules))
    .unwrap();
    crate::configuration::Evaluator::new(vec![(crate::policy::Scope::Project, snapshot)]).unwrap()
}

#[test]
fn native_sequence_diagnostics_share_parser_owned_coverage_and_decisions() {
    use crate::{
        policy::normalized::{Context, Input},
        shell::{self, Dialect},
    };
    let evaluator = evaluator();
    let root = crate::test_support::absolute("/acme");
    for (dialect, source, expected, admitted) in [
        (Dialect::Powershell, "git status; git push", "deny", true),
        (Dialect::Powershell, "& 'git' status", "allow", true),
        (Dialect::Cmd, "git status & git push", "deny", true),
        (
            Dialect::Cmd,
            "git status && git push || git status",
            "deny",
            true,
        ),
        (Dialect::Cmd, "node -e dangerous", "ask", false),
        (
            Dialect::Powershell,
            "git status; Remove-Item notes.txt",
            "ask",
            false,
        ),
        (Dialect::Unsupported, "git status", "ask", false),
    ] {
        let analysis = shell::analyze(source, dialect, &root, &root, &evaluator).unwrap();
        assert_eq!(analysis["decision"], expected, "{source}: {analysis}");
        assert_eq!(shell::supported_dialect(&analysis), admitted, "{analysis}");
        assert_eq!(shell::opaque_native(&analysis), !admitted, "{analysis}");
        let input = Input::Shell {
            source: source.into(),
            dialect: serde_json::to_value(dialect)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            dialect_version: None,
            original_input: serde_json::json!({"command":source}),
        };
        let diagnostic = evaluator
            .evaluate_normalized(
                &input,
                &Context {
                    cwd: root.to_string(),
                    environment: None,
                    working_files: None,
                },
                &root,
                None,
                "2026-10-30T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(
            diagnostic["decision"], analysis["decision"],
            "{source}: {diagnostic}"
        );
        assert_eq!(
            shell::supported_dialect(&diagnostic["analysis_coverage"]),
            admitted
        );
    }
}

#[test]
fn reported_shell_version_is_retained_without_granting_version_specific_syntax() {
    use crate::shell::{self, Dialect};
    let evaluator = evaluator();
    let root = crate::test_support::absolute("/acme");
    let analysis = shell::analyze_versioned_with_working_files(
        "git status",
        Dialect::Powershell,
        Some("7.5.0"),
        &root,
        None,
        &root,
        &evaluator,
        &|| Ok(()),
        None,
    )
    .unwrap();
    assert_eq!(analysis["dialect_version"], "7.5.0");
    assert_eq!(analysis["source_units"][0]["dialect_version"], "7.5.0");
    let unknown = shell::analyze_versioned_with_working_files(
        "git status && git push",
        Dialect::Powershell,
        Some("7.5.0"),
        &root,
        None,
        &root,
        &evaluator,
        &|| Ok(()),
        None,
    )
    .unwrap();
    assert!(!shell::supported_dialect(&unknown));
    assert!(matches!(
        shell::validate_version(Some(&"v".repeat(65))),
        Err(Error::Contract(_))
    ));
    assert!(matches!(
        shell::validate_version(Some("7\n5")),
        Err(Error::Contract(_))
    ));
}
