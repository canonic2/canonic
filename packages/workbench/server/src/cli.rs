//! CLI arguments and bounded stdin/stdout JSON transport.
use crate::{config, index, model::Request, serve};
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};

const USAGE: &str = "expected: workbench-server serve --root <project> --product <folder> [--ports <port,...>], workbench-server <init|list|check|build> <project> [build-output] [--node <program>] [--product <folder>], workbench-server index < request.json, workbench-server config <folder> [--space <id>], or workbench-server spaces <folder>...";

pub(crate) fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("index") => index_once(),
        Some("config") => config_once(args),
        Some("spaces") => spaces_once(args),
        Some("serve") => serve::run(serve::Options::parse(args)?),
        Some(command @ ("init" | "list" | "check" | "build")) => {
            if !crate::build::run(command, args)? {
                std::process::exit(1);
            }
            Ok(())
        }
        _ => bail!(USAGE),
    }
}

/// The resolved config of a folder's space, as JSON on stdout.
fn config_once(mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(dir) = args.next() else { bail!(USAGE) };
    let key = match (args.next().as_deref(), args.next()) {
        (None, None) => None,
        (Some("--space"), Some(key)) => Some(key),
        _ => bail!(USAGE),
    };
    let dir = std::path::absolute(&dir).with_context(|| format!("invalid folder {dir}"))?;
    let answer = config::answer(&dir, key.as_deref())?;
    std::io::stdout().write_all(&serde_json::to_vec(&answer)?)?;
    Ok(())
}

/// The spaces the folders' workbench.yaml files hold, as JSON on stdout.
fn spaces_once(args: impl Iterator<Item = String>) -> Result<()> {
    let folders = args
        .map(|dir| {
            let absolute =
                std::path::absolute(&dir).with_context(|| format!("invalid folder {dir}"))?;
            Ok(crate::paths::resolve(&absolute, ""))
        })
        .collect::<Result<Vec<_>>>()?;
    if folders.is_empty() {
        bail!(USAGE);
    }
    let answer = crate::spaces::listed(&folders);
    std::io::stdout().write_all(&serde_json::to_vec(&answer)?)?;
    Ok(())
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
