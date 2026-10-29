//! Protected file access settings and noninteractive current-state reads.
use crate::review::{Error, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::path::Path;

/// Reading agent settings files, which may hold credentials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Read {
    /// Ask where the host can prompt, otherwise block.
    #[default]
    Ask,
    Allow,
}

/// Changing a group of protected files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Change {
    #[default]
    Block,
    Allow,
}

/// What agents may do with protected files beyond the defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathAccess {
    /// `.mcp.json` and each agent's hook and MCP settings files.
    #[serde(default)]
    pub(crate) agent_settings_read: Read,
    #[serde(default)]
    pub(crate) agent_settings_change: Change,
    /// `.shield` policy files other than review state, caches and `.gitignore`.
    #[serde(default)]
    pub(crate) shield_config_change: Change,
}

/// The stored choice; absent means the defaults, and an invalid record is an error.
pub(crate) fn load(meta: &Value) -> Result<PathAccess, Error> {
    match meta.get("path_access") {
        None | Some(Value::Null) => Ok(PathAccess::default()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| Error::Invalid("invalid protected path access record")),
    }
}

/// The current choice, read under the project state lock.
pub(crate) fn current(root: &Path) -> Result<PathAccess, Error> {
    let context = crate::review::open(root)?;
    Store::open(root)?.inspect_checkpoint(&context, &|| Ok(()), |state| load(&state.operator))
}

#[cfg(test)]
pub(crate) fn configure_fixture(root: &Path, access: PathAccess) {
    store_fixture(root, json!(access));
}

/// Stores a raw record, including invalid ones, without review.
#[cfg(test)]
pub(crate) fn store_fixture(root: &Path, record: Value) {
    let context = crate::review::open(root).unwrap();
    Store::open(root)
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["path_access"] = record;
            Ok(json!({}))
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_record_is_default_and_invalid_record_is_refused() {
        assert_eq!(load(&json!({})).unwrap(), PathAccess::default());
        assert_eq!(
            load(&json!({"path_access":null})).unwrap(),
            PathAccess::default()
        );
        assert_eq!(
            load(&json!({"path_access":{"agent_settings_read":"allow"}})).unwrap(),
            PathAccess {
                agent_settings_read: Read::Allow,
                ..PathAccess::default()
            }
        );
        for invalid in [
            json!({"path_access":{"agent_settings_read":"block"}}),
            json!({"path_access":{"review_state_change":"allow"}}),
            json!({"path_access":"allow"}),
        ] {
            assert!(load(&invalid).is_err(), "{invalid}");
        }
    }
}
