//! Exact ownership of the single Shield entry in JSON MCP settings.
use crate::review::Error;
use crate::{agents::SettingsUpdate, mcp_connections::Transport};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Copy)]
pub(super) struct Http {
    pub key: &'static str,
    pub legacy: Option<&'static str>,
    pub discriminator: Option<(&'static str, &'static str)>,
}

fn document(source: Option<&str>) -> Result<Value, Error> {
    let value = super::setup::parse(source)?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(Error::Invalid("invalid MCP configuration shape"));
    }
    Ok(value)
}

pub(super) fn present(source: &str) -> Result<bool, Error> {
    Ok(document(Some(source))?["mcpServers"]
        .get("shield")
        .is_some())
}

pub(super) fn merge(
    source: Option<&str>,
    old: Option<&str>,
    new: Option<&str>,
) -> Result<String, Error> {
    let mut config = document(source)?;
    let decode = |source: &str| {
        crate::json::parse(source.as_bytes()).map_err(|_| Error::Invalid("invalid owned MCP entry"))
    };
    let old = old.map(decode).transpose()?;
    let new = new.map(decode).transpose()?;
    let servers = config
        .as_object_mut()
        .expect("checked object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("checked server map");
    if let Some(actual) = servers.get("shield")
        && Some(actual) != old.as_ref()
    {
        return Err(Error::Invalid(
            "Shield MCP entry is unowned or manually changed",
        ));
    }
    servers.remove("shield");
    if let Some(new) = new {
        servers.insert("shield".into(), new);
    }
    if servers.is_empty() {
        config.as_object_mut().expect("object").remove("mcpServers");
    }
    serde_json::to_string_pretty(&config)
        .map(|s| format!("{s}\n"))
        .map_err(|_| Error::Invalid("MCP settings encoding"))
}

pub(super) fn connection(
    source: &str,
    alias: &str,
    http: Http,
) -> Result<Option<Transport>, Error> {
    let config = document(Some(source))?;
    // These hosts supply explicit server identity in their hook envelope;
    // underscore or punctuation normalization does not select the connection.
    config["mcpServers"]
        .get(alias)
        .map(|entry| decode(entry, http))
        .transpose()
}

fn decode(entry: &Value, http: Http) -> Result<Transport, Error> {
    let mut value = entry
        .as_object()
        .cloned()
        .ok_or(Error::Invalid("MCP entry must be an object"))?;
    let key = if value.contains_key(http.key) {
        Some(http.key)
    } else {
        http.legacy.filter(|key| value.contains_key(*key))
    };
    if let Some(key) = key {
        let url = value
            .remove(key)
            .ok_or(Error::Invalid("invalid MCP endpoint"))?;
        if let Some((tag, expected)) = http.discriminator {
            match value.remove(tag) {
                Some(actual) if actual == expected => {}
                None if Some(key) == http.legacy => {}
                _ => {
                    return Err(Error::Invalid(
                        "MCP endpoint requires an explicit Streamable HTTP transport",
                    ));
                }
            }
        }
        if !value.is_empty() {
            return Err(Error::Invalid(
                "unsupported or mixed MCP transport settings",
            ));
        }
        return url
            .as_str()
            .map(|url| Transport::Http { url: url.into() })
            .ok_or(Error::Invalid("invalid MCP endpoint"));
    }
    if value
        .keys()
        .any(|key| !["command", "args", "env"].contains(&key.as_str()))
    {
        return Err(Error::Invalid("unsupported MCP transport settings"));
    }
    value.insert("type".into(), json!("stdio"));
    serde_json::from_value(Value::Object(value))
        .map_err(|_| Error::Invalid("invalid MCP process settings"))
}

fn encode(transport: &Transport, http: Http) -> Result<Value, Error> {
    Ok(match transport {
        Transport::Stdio { command, args, env } => json!({"command":command,"args":args,"env":env}),
        Transport::Http { url } => {
            let mut value = json!({http.key:url});
            if let Some((tag, kind)) = http.discriminator {
                value[tag] = json!(kind);
            }
            value
        }
        Transport::Defaults { .. } => {
            return Err(Error::Invalid(
                "MCP defaults must be resolved before projection",
            ));
        }
    })
}

pub(super) fn connections(
    source: &str,
    previous: &Value,
    desired: &BTreeMap<String, Transport>,
    http: Http,
) -> Result<SettingsUpdate, Error> {
    connections_with(
        source,
        previous,
        desired,
        |entry| decode(entry, http),
        |transport| encode(transport, http),
        |_, _| Ok(()),
    )
}

/// Shared exact ownership; codecs and alias identity remain adapter-owned.
pub(super) fn connections_with(
    source: &str,
    previous: &Value,
    desired: &BTreeMap<String, Transport>,
    decode: impl Fn(&Value) -> Result<Transport, Error>,
    encode: impl Fn(&Transport) -> Result<Value, Error>,
    check_alias: impl Fn(&str, &serde_json::Map<String, Value>) -> Result<(), Error>,
) -> Result<SettingsUpdate, Error> {
    let mut config = document(Some(source))?;
    let servers = config
        .as_object_mut()
        .expect("object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("server map");
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
        check_alias(alias, servers)?;
        if let Some(existing) = servers.get(alias) {
            if &decode(existing)? != transport {
                return Err(Error::Invalid(
                    "MCP alias is already configured differently; resolve before setup",
                ));
            }
        } else {
            let entry = encode(transport)?;
            servers.insert(alias.clone(), entry.clone());
            owned.insert(alias.clone(), entry);
        }
    }
    Ok(SettingsUpdate {
        source: format!(
            "{}\n",
            serde_json::to_string_pretty(&config)
                .map_err(|_| Error::Invalid("MCP settings encoding"))?
        ),
        ownership: Some(Value::Object(owned)),
    })
}

pub(super) fn check_scope(path: &Path, alias: &str) -> Result<(), Error> {
    if let Some(source) = super::mcp_scopes::read(path)? {
        let config = document(Some(&source))?;
        if config["mcpServers"].get(alias).is_some() {
            return Err(Error::Invalid(
                "bound MCP alias is shadowed by another host scope; resolve the conflict before setup",
            ));
        }
    }
    Ok(())
}
