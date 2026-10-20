//! A space's configuration: its workbench.yaml and workbench.local.yaml read,
//! checked and resolved for this machine, giving the same answer as the Node
//! reader (`server/node/config.js` with `contracts/manifest.js`). Parsing and
//! rules are pure; `read` alone touches the filesystem.
pub(crate) mod edit;
mod js;
mod manifest;
mod read;
mod sizes;
mod yaml;

pub(crate) use read::{identity, spaces_in};
pub(crate) use sizes::page_sizes;

use anyhow::{Result, bail};
use std::path::Path;

/// An object’s keys in JavaScript’s order, as the browser reads the answer.
pub(crate) fn ordered(map: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    serde_json::Value::Object(js::ordered(map))
}

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
