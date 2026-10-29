//! Pack removal withdraws associations while retaining independent servers.
use super::Error;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) fn retain(before: &Value, config: &mut Value) -> Result<(), Error> {
    let selected: BTreeSet<String> = config["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let id = p["id"].as_str()?;
            Some(
                p["registry_identity"]
                    .as_str()
                    .map_or_else(|| id.into(), |registry| format!("{registry}/{id}")),
            )
        })
        .collect();
    let needs_defaults = ["mcp", "local_mcp"].iter().any(|field| {
        config[*field]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(_, c)| {
                c["transport"]["type"] == "defaults"
                    && c["transport"]["pack"]
                        .as_str()
                        .is_some_and(|id| !selected.contains(id))
            })
    });
    let old = if needs_defaults {
        Some(crate::configuration::Evaluator::new(vec![(
            crate::policy::Scope::Project,
            crate::configuration::Bundle::snapshot_from_json(
                &serde_json::to_vec(before).map_err(|_| Error::Invalid("MCP source encoding"))?,
            )?,
        )])?)
    } else {
        None
    };
    let mut local_moves = Vec::new();
    for field in ["mcp", "local_mcp"] {
        let Some(connections) = config.get_mut(field).and_then(Value::as_object_mut) else {
            continue;
        };
        let mut removed = Vec::new();
        for (id, c) in connections.iter_mut() {
            if let Some(packs) = c.get_mut("packs").and_then(Value::as_object_mut) {
                packs.retain(|pack, _| selected.contains(pack));
            }
            if c["transport"]["type"] == "defaults"
                && c["transport"]["pack"]
                    .as_str()
                    .is_some_and(|pack| !selected.contains(pack))
            {
                let effective = old
                    .as_ref()
                    .and_then(|e| e.effective().mcp_connections().get(id))
                    .ok_or(Error::Invalid("removed pack MCP defaults are unavailable"))?;
                c["transport"] = json!(effective.transport);
                if field == "mcp" && effective.local {
                    local_moves.push((id.clone(), c.clone()));
                    removed.push(id.clone());
                }
            }
        }
        for id in removed {
            connections.remove(&id);
        }
    }
    for (id, c) in local_moves {
        if !config["local_mcp"].is_object() {
            config["local_mcp"] = json!({});
        }
        // A preexisting complete local override already supplies this connection.
        if config["local_mcp"].get(&id).is_none() {
            config["local_mcp"][&id] = c;
        }
    }
    Ok(())
}
