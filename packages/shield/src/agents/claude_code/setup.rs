//! Scoped Claude MCP approval settings and ownership of inserted list entries.
use crate::{agents::SettingsUpdate, review::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const APPLY: &str = "mcp__shield__policy_apply";
const SERVER: &str = "shield";

#[cfg(test)]
mod tests;

/// Claude's permission rule for one Shield tool.
fn rule(tool: &str) -> String {
    format!("mcp__shield__{tool}")
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum List {
    Existing,
    Created,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    allow: Option<List>,
    /// Allow rules Shield added; rules the user already had are never owned.
    #[serde(default)]
    rules: Vec<String>,
    ask: Option<List>,
    enabled: Option<List>,
    permissions_created: bool,
}

/// Install with `approved` tools allowed without asking, or remove with `None`.
pub(super) fn merge(
    source: Option<&str>,
    owned: Option<&Value>,
    approved: Option<&[String]>,
) -> Result<SettingsUpdate, Error> {
    let install = approved.is_some();
    let desired: Vec<String> = approved
        .unwrap_or_default()
        .iter()
        .map(|t| rule(t))
        .collect();
    let mut config = super::super::setup::parse(source)?;
    let object = config
        .as_object_mut()
        .ok_or(Error::Invalid("Claude settings must be an object"))?;
    let mut ownership: Ownership = owned
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| Error::Invalid("invalid owned Claude settings"))?
        .unwrap_or_default();
    if install && !object.contains_key("permissions") {
        object.insert("permissions".into(), json!({}));
        ownership.permissions_created = true;
    }
    if let Some(permissions) = object.get_mut("permissions") {
        let permissions = permissions
            .as_object_mut()
            .ok_or(Error::Invalid("Claude permissions must be an object"))?;
        (ownership.allow, ownership.rules) =
            merge_rules(permissions, &desired, &ownership.rules, ownership.allow)?;
        // Remove only an ask entry inserted by an earlier Shield setup.
        // Confirmation is owned by the MCP server, independently of tool mode.
        merge_list(permissions, "ask", APPLY, ownership.ask, false)?;
        ownership.ask = None;
        if !install && ownership.permissions_created && permissions.is_empty() {
            object.remove("permissions");
        }
    }
    ownership.enabled = merge_list(
        object,
        "enabledMcpjsonServers",
        SERVER,
        ownership.enabled,
        install,
    )?;
    Ok(SettingsUpdate {
        source: format!(
            "{}\n",
            serde_json::to_string_pretty(&config)
                .map_err(|_| Error::Invalid("Claude settings encoding"))?
        ),
        ownership: install
            .then(|| serde_json::to_value(ownership))
            .transpose()
            .map_err(|_| Error::Invalid("Claude settings ownership encoding"))?,
    })
}

/// Reconcile `permissions.allow` with the desired rules: drop owned rules no
/// longer desired, append missing ones and own only what was appended.
fn merge_rules(
    permissions: &mut Map<String, Value>,
    desired: &[String],
    owned: &[String],
    list: Option<List>,
) -> Result<(Option<List>, Vec<String>), Error> {
    let created = !permissions.contains_key("allow");
    if created && !desired.is_empty() {
        permissions.insert("allow".into(), json!([]));
    }
    let Some(value) = permissions.get_mut("allow") else {
        return Ok((list, Vec::new()));
    };
    let values = value
        .as_array_mut()
        .filter(|v| v.iter().all(Value::is_string))
        .ok_or(Error::Invalid(
            "Claude MCP approval settings must be string arrays",
        ))?;
    values.retain(|v| !owned.iter().any(|r| v == r && !desired.contains(r)));
    let mut now_owned = Vec::new();
    for rule in desired {
        if !values.iter().any(|v| v == rule) {
            values.push(json!(rule));
            now_owned.push(rule.clone());
        } else if owned.contains(rule) {
            now_owned.push(rule.clone());
        }
    }
    let list = list.or(if created {
        Some(List::Created)
    } else {
        Some(List::Existing)
    });
    if matches!(list, Some(List::Created)) && values.is_empty() {
        permissions.remove("allow");
        return Ok((None, now_owned));
    }
    Ok((list.filter(|_| !now_owned.is_empty()), now_owned))
}

fn merge_list(
    object: &mut Map<String, Value>,
    key: &str,
    entry: &str,
    owned: Option<List>,
    install: bool,
) -> Result<Option<List>, Error> {
    let created = !object.contains_key(key);
    if install && created {
        object.insert(key.into(), json!([]));
    }
    let Some(value) = object.get_mut(key) else {
        return Ok(owned);
    };
    let values = value
        .as_array_mut()
        .filter(|v| v.iter().all(Value::is_string))
        .ok_or(Error::Invalid(
            "Claude MCP approval settings must be string arrays",
        ))?;
    let position = values.iter().position(|v| v == entry);
    if install {
        if position.is_none() {
            values.push(json!(entry));
            return Ok(owned.or(Some(if created {
                List::Created
            } else {
                List::Existing
            })));
        }
    } else if owned.is_some() {
        if let Some(index) = position {
            values.remove(index);
        }
        if matches!(owned, Some(List::Created)) && values.is_empty() {
            object.remove(key);
        }
    }
    Ok(owned)
}

/// Approved tools are the operator's choice, so only server approval is checked.
pub(super) fn diagnose(config: &Value, diagnostics: &mut Vec<&'static str>) {
    if !config["enabledMcpjsonServers"]
        .as_array()
        .is_some_and(|v| v.iter().any(|v| v == SERVER))
    {
        diagnostics.push("Shield project MCP server needs approval in Claude");
    }
}
