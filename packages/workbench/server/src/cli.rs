//! CLI arguments and bounded stdin/stdout JSON transport.
use crate::{index, model::Request, serve};
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};

const USAGE: &str = "expected: workbench-server index < request.json, or workbench-server serve --upstream <port> [--ports <port,...>]";

pub(crate) fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("index") => index_once(),
        Some("serve") => serve::run(serve::Options::parse(args)?),
        _ => bail!(USAGE),
    }
}

/// One index transaction: a request on stdin, the answer on stdout.
fn index_once() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)?;
    let request: Request = serde_json::from_str(&input).context("invalid index request")?;
    let answer = index::answer(&request)?;
    std::io::stdout().write_all(&serde_json::to_vec(&answer)?)?;
    Ok(())
}
