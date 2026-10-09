//! Parses local gallery options and delegates terminal ownership to the library.

use canonic_rust_tui::gallery::{self, Cli};
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    match gallery::run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tui-gallery: {error:#}");
            ExitCode::FAILURE
        }
    }
}
