//! Native Workbench page indexing; one JSON request and response per process.
mod definition;
mod discovery;
mod evaluate;
mod index;
mod literal;
mod saved;

use anyhow::{Context, Result, ensure};
use std::io::{Read, Write};

/// Run the `index` command using a JSON request on stdin.
///
/// # Errors
/// Returns a failure for invalid requests, discovery or evaluation failures.
pub fn run() -> Result<()> {
    ensure!(
        std::env::args().nth(1).as_deref() == Some("index"),
        "expected: workbench-native index < request.json"
    );
    let mut input = String::new();
    std::io::stdin().take(1024 * 1024).read_to_string(&mut input)?;
    let request = serde_json::from_str(&input).context("invalid index request")?;
    let answer = index::index(&request)?;
    std::io::stdout().write_all(&serde_json::to_vec(&answer)?)?;
    Ok(())
}
