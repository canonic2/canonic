//! A space's configuration: its workbench.yaml and workbench.local.yaml read,
//! checked and resolved for this machine, giving the same answer as the Node
//! reader (`server/node/config.js` with `contracts/manifest.js`). Parsing and
//! rules are pure; `read` alone touches the filesystem. Phase 2 of the
//! server's move into Rust (specs/server.md).
mod js;
mod manifest;
mod read;
mod sizes;
mod yaml;

use anyhow::{Result, bail};
use std::path::Path;

/// The resolved config of a space, as JSON, or `null` without a workbench.yaml.
///
/// # Errors
/// Returns the Node reader's message when a file can't be read or parsed.
pub(crate) fn answer(dir: &Path, key: Option<&str>) -> Result<serde_json::Value> {
    match read::resolved(dir, key) {
        Ok(found) => Ok(found.unwrap_or(serde_json::Value::Null)),
        Err(message) => bail!(message),
    }
}
