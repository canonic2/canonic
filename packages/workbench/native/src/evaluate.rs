//! Batched JavaScript evaluation and custom picomatch discovery adapter.
use crate::index::Request;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Deserialize)]
pub(crate) struct Evaluated {
    pub(crate) file: String,
    pub(crate) entry: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
    pub(crate) docs: bool,
    pub(crate) files: Vec<String>,
    #[serde(default)]
    pub(crate) cacheable: bool,
}

#[derive(Deserialize)]
pub(crate) struct Batch {
    pub(crate) entries: Vec<Evaluated>,
    pub(crate) settings: Vec<String>,
}

pub(crate) fn call(request: &Request, action: &str, files: &[String]) -> Result<serde_json::Value> {
    let mut child = Command::new(&request.node)
        .arg(&request.evaluator)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .env_remove("NODE_OPTIONS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("start definition evaluator")?;
    let input =
        json!({"root": request.root, "options": request.options, "action": action, "files": files});
    let write = child
        .stdin
        .take()
        .context("evaluator stdin")?
        .write_all(&serde_json::to_vec(&input)?);
    if let Err(error) = write {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let output = child
        .wait_with_output()
        .context("definition evaluator output")?;
    ensure!(
        output.status.success(),
        "definition evaluator failed: {}",
        output.status
    );
    serde_json::from_slice(&output.stdout).context("invalid evaluator JSON")
}
