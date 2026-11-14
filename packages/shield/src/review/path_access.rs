//! Protected file access: the `protection` key of the policy files, and its
//! noninteractive current-state read.
use crate::review::{Error, storage::Store};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) use crate::configuration::settings::{Change, Read};

/// What agents may do with protected files beyond the defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathAccess {
    /// `.mcp.json` and each agent's hook and MCP settings files.
    #[serde(default)]
    pub(crate) agent_settings_read: Read,
    #[serde(default)]
    pub(crate) agent_settings_change: Change,
    /// Earlier releases let the operator open `.shield` policy files to agent
    /// edits. Those files are the enforced policy, so agent writes to them are
    /// always blocked; a saved record that still names the choice loads, and
    /// its value is ignored and never written again.
    #[serde(default, rename = "shield_config_change", skip_serializing)]
    pub(crate) retired_policy_file_change: Option<Change>,
}

/// The access in effect, read under the project state lock. Policy files that
/// cannot be read give the defaults.
pub(crate) fn current(root: &Path) -> Result<PathAccess, Error> {
    let context = crate::review::open(root)?;
    Store::open(root)?.inspect_checkpoint(&context, &|| Ok(()), |state| {
        Ok(super::settings::path_access(&state.settings))
    })
}

/// Set the access in `config.local.yml`, as `shield protection set` does.
#[cfg(test)]
pub(crate) fn configure_fixture(root: &Path, access: PathAccess) {
    let context = crate::review::open(root).unwrap();
    super::settings::write(&context, &super::settings::protection(access)).unwrap();
}

/// Store a raw access record the way an earlier release did, in the
/// checkpoint's operator metadata, including invalid ones.
#[cfg(test)]
pub(crate) fn store_fixture(root: &Path, record: serde_json::Value) {
    let context = crate::review::open(root).unwrap();
    Store::open(root)
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["path_access"] = record;
            Ok(serde_json::json!({}))
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_earlier_policy_file_choice_loads_but_is_never_written_again() {
        let saved: PathAccess = serde_json::from_value(json!({
            "agent_settings_change":"allow","shield_config_change":"allow"
        }))
        .unwrap();
        assert_eq!(saved.agent_settings_change, Change::Allow);
        assert!(json!(saved).get("shield_config_change").is_none());
    }
}
