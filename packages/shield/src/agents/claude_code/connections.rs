//! Claude's connection entries and project enablement, with exact ownership.
use crate::{agents::SettingsUpdate, mcp_connections::Transport, review::Error};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn merge(
    source: &str,
    previous: &Value,
    desired: &BTreeMap<String, Transport>,
) -> Result<SettingsUpdate, Error> {
    let mut document =
        crate::json::parse(source.as_bytes()).map_err(|_| Error::Invalid("invalid MCP JSON"))?;
    let servers = document
        .as_object_mut()
        .ok_or(Error::Invalid("MCP configuration must be an object"))?
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(Error::Invalid("mcpServers must be an object"))?;
    for (alias, entry) in previous.as_object().into_iter().flatten() {
        if servers.get(alias).is_some_and(|actual| actual != entry) {
            return Err(Error::Invalid(
                "owned MCP connection was changed; resolve before setup",
            ));
        }
        servers.remove(alias);
    }
    let mut owned = serde_json::Map::new();
    for (alias, transport) in desired {
        super::super::unambiguous_mcp_alias(alias, servers.keys().map(String::as_str))?;
        let entry = serde_json::to_value(transport)
            .map_err(|_| Error::Invalid("MCP connection encoding"))?;
        if let Some(existing) = servers.get(alias) {
            let mut existing = existing.clone();
            if existing.get("type").is_none() {
                existing["type"] = json!("stdio");
            }
            if serde_json::from_value::<Transport>(existing).ok().as_ref() != Some(transport) {
                return Err(Error::Invalid(
                    "MCP alias is already configured differently; resolve before setup",
                ));
            }
        } else {
            servers.insert(alias.clone(), entry.clone());
            owned.insert(alias.clone(), entry);
        }
    }
    Ok(SettingsUpdate {
        source: format!(
            "{}\n",
            serde_json::to_string_pretty(&document).map_err(|_| Error::Invalid("MCP encoding"))?
        ),
        ownership: Some(Value::Object(owned)),
    })
}

pub(super) fn settings(
    source: &str,
    previous: &Value,
    aliases: &[String],
) -> Result<SettingsUpdate, Error> {
    let mut document = crate::json::parse(source.as_bytes())
        .map_err(|_| Error::Invalid("invalid Claude settings JSON"))?;
    let fields = document
        .as_object_mut()
        .ok_or(Error::Invalid("Claude settings must be an object"))?;
    if fields.get("disabledMcpjsonServers").is_some_and(|v| {
        v.as_array().is_none_or(|disabled| {
            disabled
                .iter()
                .any(|v| aliases.iter().any(|alias| v == alias))
        })
    }) {
        return Err(Error::Invalid(
            "a bound MCP alias is disabled in Claude settings; resolve before setup",
        ));
    }
    let enabled = fields
        .entry("enabledMcpjsonServers")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(Error::Invalid("invalid MCP enablement list"))?;
    if enabled.iter().any(|v| !v.is_string()) {
        return Err(Error::Invalid("invalid MCP enablement list"));
    }
    let old: BTreeSet<_> = previous
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    enabled.retain(|v| !old.contains(v.as_str().expect("string checked")));
    let mut added = Vec::new();
    for alias in aliases {
        if !enabled.iter().any(|v| v == alias) {
            enabled.push(json!(alias));
            added.push(alias);
        }
    }
    Ok(SettingsUpdate {
        source: format!(
            "{}\n",
            serde_json::to_string_pretty(&document)
                .map_err(|_| Error::Invalid("Claude encoding"))?
        ),
        ownership: Some(json!(added)),
    })
}
