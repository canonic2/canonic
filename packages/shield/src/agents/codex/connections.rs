//! Codex connection projection retains exact owned tables and native prompts.
use crate::{agents::SettingsUpdate, mcp_connections::Transport, review::Error};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt::Write as _};
use toml_edit::{DocumentMut, Item, Table};

pub(super) fn merge(
    source: &str,
    previous: &Value,
    desired: &BTreeMap<String, Transport>,
) -> Result<SettingsUpdate, Error> {
    let mut doc: DocumentMut = source
        .parse()
        .map_err(|_| Error::Invalid("invalid Codex configuration"))?;
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or(Error::Invalid("Codex mcp_servers must be a table"))?;
    for (alias, entry) in previous.as_object().into_iter().flatten() {
        if servers
            .get(alias)
            .is_some_and(|actual| entry.as_str() != Some(actual.to_string().as_str()))
        {
            return Err(Error::Invalid(
                "owned Codex MCP connection changed; resolve before setup",
            ));
        }
        servers.remove(alias);
    }
    let mut owned = serde_json::Map::new();
    for (alias, transport) in desired {
        if servers.contains_key(alias) {
            continue;
        }
        let mut text = String::new();
        match transport {
            Transport::Defaults { .. } => return Err(Error::Invalid("unresolved MCP defaults")),
            Transport::Http { url } => {
                writeln!(text, "url = {}", json!(url)).expect("writing to String");
            }
            Transport::Stdio { command, args, env } => {
                write!(
                    text,
                    "command = {}\nargs = {}\n",
                    json!(command),
                    json!(args)
                )
                .expect("writing to String");
                if !env.is_empty() {
                    writeln!(
                        text,
                        "env = {{ {} }}",
                        env.iter()
                            .map(|(k, v)| format!("{} = {}", json!(k), json!(v)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .expect("writing to String");
                }
            }
        }
        text.push_str("default_tools_approval_mode = \"prompt\"\n");
        let parsed: DocumentMut = text
            .parse()
            .map_err(|_| Error::Invalid("Codex MCP encoding"))?;
        servers.insert(alias, Item::Table(parsed.as_table().clone()));
        owned.insert(
            alias.clone(),
            json!(servers.get(alias).expect("entry inserted").to_string()),
        );
    }
    let output = doc.to_string();
    // Existing manual aliases may be bound, but never silently adopted.
    for (alias, transport) in desired {
        if super::setup::connection(&output, alias)?.as_ref() != Some(transport) {
            return Err(Error::Invalid(
                "Codex MCP alias is configured differently; resolve before setup",
            ));
        }
    }
    Ok(SettingsUpdate {
        source: output,
        ownership: Some(Value::Object(owned)),
    })
}
