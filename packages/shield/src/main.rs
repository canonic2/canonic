//! Parses CLI arguments and enters the Shield library.

use clap::Parser;

fn main() -> std::process::ExitCode {
    shield::run(&shield::cli::Cli::parse())
}
