//! CLI startup diagnostics and exit policy; the binary owns the Tokio runtime.
use super::{CommandLine, presentation};
use crate::error::{Error, Result};
use clap::Parser;
use std::{ffi::OsString, process::ExitCode};

#[derive(Debug)]
enum ParseFailure {
    Help(clap::Error),
    Arguments { error: Error, machine: bool },
}

/// Parse process arguments and print help or a complete argument diagnostic.
///
/// # Errors
/// Returns the process status when no command should run, including help.
pub fn parse() -> std::result::Result<CommandLine, ExitCode> {
    match parse_from(std::env::args_os().collect()) {
        Ok(input) => Ok(input),
        Err(ParseFailure::Help(error)) => Err(if error.print().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }),
        Err(ParseFailure::Arguments { error, machine }) => {
            let _ = presentation::print_error(&error, machine);
            Err(ExitCode::from(2))
        }
    }
}

fn parse_from(args: Vec<OsString>) -> std::result::Result<CommandLine, ParseFailure> {
    // Clap cannot return parsed globals on failure. Inspect the same argument
    // snapshot, respecting the end-of-options marker, to choose the diagnostic.
    let machine = args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json");
    CommandLine::try_parse_from(args).map_err(|error| match error.kind() {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
            ParseFailure::Help(error)
        }
        _ => ParseFailure::Arguments {
            error: Error::invalid(error.to_string()),
            machine,
        },
    })
}

/// Run a parsed command, present any unreported error and select its exit status.
pub async fn run_to_exit(input: CommandLine) -> ExitCode {
    let machine = input.machine_output();
    let result = super::run(input).await;
    if let Err(error) = &result
        && !presentation::broken_pipe(error)
    {
        let _ = presentation::print_error(error, machine);
    }
    exit_code(&result)
}

fn exit_code(result: &Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if presentation::broken_pipe(error) => ExitCode::SUCCESS,
        Err(error) if error.code == "SETUP_CANCELLED" => ExitCode::from(130),
        Err(error)
            if matches!(
                error.code.as_str(),
                "INVALID_ARGUMENT" | "invalid_arguments"
            ) =>
        {
            ExitCode::from(2)
        }
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        std::iter::once("testrun")
            .chain(words.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn valid_commands_preserve_explicit_and_hidden_machine_output() {
        for (words, machine) in [
            (vec!["mcp", "serve", "--check"], false),
            (vec!["--json", "mcp", "serve", "--check"], true),
            (vec!["mcp", "serve", "--check", "--json"], true),
            (vec!["close", "--endpoint", "/tmp/acme/control.sock"], true),
        ] {
            assert_eq!(parse_from(args(&words)).unwrap().machine_output(), machine);
        }
    }

    #[test]
    fn help_and_version_are_distinct_from_argument_errors() {
        for words in [vec!["--help"], vec!["--version"], vec!["mcp", "--help"]] {
            assert!(matches!(
                parse_from(args(&words)),
                Err(ParseFailure::Help(_))
            ));
        }
    }

    #[test]
    fn argument_diagnostics_respect_json_and_the_end_of_options() {
        for (words, expected) in [
            (vec!["--unknown-acme-flag"], false),
            (vec!["--json", "--unknown-acme-flag"], true),
            (vec!["--unknown-acme-flag", "--json"], true),
            (vec!["--unknown-acme-flag", "--", "--json"], false),
        ] {
            let Err(ParseFailure::Arguments { error, machine }) = parse_from(args(&words)) else {
                panic!("Expected an argument diagnostic");
            };
            assert_eq!(error.code, "INVALID_ARGUMENT");
            assert!(error.message.contains("--unknown-acme-flag"));
            assert_eq!(machine, expected);
        }
    }

    #[test]
    fn exit_policy_keeps_cancellation_failed_results_and_closed_pipes_distinct() {
        assert_eq!(exit_code(&Ok(())), ExitCode::SUCCESS);
        for (code, status) in [
            ("CLI_OUTPUT_CLOSED", ExitCode::SUCCESS),
            ("SETUP_CANCELLED", ExitCode::from(130)),
            ("CLI_RESULT_FAILED", ExitCode::FAILURE),
            ("HOST_IO", ExitCode::FAILURE),
            ("INVALID_ARGUMENT", ExitCode::from(2)),
        ] {
            assert_eq!(exit_code(&Err(Error::new(code, "Acme diagnostic"))), status);
        }
    }
}
