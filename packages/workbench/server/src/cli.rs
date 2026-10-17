//! CLI arguments and bounded stdin/stdout JSON transport.
use crate::index;
use anyhow::{Context, Result, ensure};
use std::io::{Read, Write};

pub(crate) fn run() -> Result<()> {
    ensure!(
        std::env::args().nth(1).as_deref() == Some("index"),
        "expected: workbench-server index < request.json"
    );
    let mut input = String::new();
    std::io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)?;
    let request: crate::model::Request =
        serde_json::from_str(&input).context("invalid index request")?;
    let evaluator = crate::evaluate::Evaluator::new(
        &request.root,
        &request.node,
        &request.evaluator,
        &request.options,
    );
    let answer = index::index(&request, &evaluator)?;
    std::io::stdout().write_all(&serde_json::to_vec(&answer)?)?;
    Ok(())
}
