//! CLI mapping into shared documentation and immutable policy diagnostics.
use crate::{
    cli::public::{DocsCommand, PolicyCommand},
    review::{Context, Error, mcp},
};
use serde_json::{Value, json};

pub(super) fn docs(command: &DocsCommand) -> Result<Value, Error> {
    let (name, args) = docs_arguments(command);
    mcp::operator_docs(name, &args)
}

pub(super) fn docs_arguments(command: &DocsCommand) -> (&'static str, Value) {
    match command {
        DocsCommand::List { cursor, limit } => (
            "docs_list",
            canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(*limit)),
        ),
        DocsCommand::Search {
            query,
            path,
            cursor,
            limit,
        } => {
            let mut args = canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(*limit));
            args["query"] = json!(query);
            if let Some(path) = path {
                args["path"] = json!(path);
            }
            ("docs_search", args)
        }
        DocsCommand::Read {
            path,
            cursor,
            limit,
        } => {
            let mut args = canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(*limit));
            args["path"] = json!(path);
            ("docs_read", args)
        }
    }
}

pub(super) fn diagnostic(context: &Context, command: &PolicyCommand) -> Result<Value, Error> {
    mcp::operator_diagnostic(
        context,
        match command {
            PolicyCommand::Evaluate { input, proposal_id } => mcp::Diagnostic::Evaluate {
                input,
                proposal_id: proposal_id.as_deref(),
            },
            PolicyCommand::Compare {
                before,
                after,
                domain,
                input,
            } => mcp::Diagnostic::Compare {
                before,
                after,
                domain,
                input: input.as_deref(),
            },
            _ => return Err(Error::Invalid("expected policy evaluate or compare")),
        },
    )
}

#[cfg(test)]
mod documentation_tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn docs_cli_maps_cursor_and_limit_to_shared_tools() {
        let command = crate::cli::public::Cli::command();
        let docs_group = command.find_subcommand("docs").unwrap();
        for (verb, positional) in [
            ("list", None),
            ("search", Some("needle")),
            ("read", Some("mcp.md")),
        ] {
            let subcommand = docs_group.find_subcommand(verb).unwrap();
            for flag in ["cursor", "limit"] {
                assert!(
                    subcommand
                        .get_arguments()
                        .any(|argument| argument.get_long() == Some(flag))
                );
            }
            let mut words = vec!["shield", "docs", verb];
            if let Some(value) = positional {
                words.push(value);
            }
            words.extend(["--cursor", "opaque", "--limit", "2"]);
            let input = crate::cli::public::Cli::try_parse_from(words.clone()).unwrap();
            let crate::cli::public::PublicCommand::Docs(docs) = input.command else {
                panic!("docs command");
            };
            let (name, arguments) = docs_arguments(&docs.command);
            assert_eq!(name, format!("docs_{verb}"));
            assert_eq!(arguments["cursor"], "opaque");
            assert_eq!(arguments["limit"], 2);
            for invalid in ["0", if verb == "read" { "401" } else { "101" }] {
                *words.last_mut().unwrap() = invalid;
                assert!(crate::cli::public::Cli::try_parse_from(words.clone()).is_err());
            }
            let mut old = vec!["shield", "docs", verb];
            if let Some(value) = positional {
                old.push(value);
            }
            old.extend([
                if verb == "read" {
                    "--start-line"
                } else {
                    "--offset"
                },
                "1",
            ]);
            assert!(crate::cli::public::Cli::try_parse_from(old).is_err());
        }
    }
}
