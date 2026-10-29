//! Claude local/user aliases must not lend project rules to another server.
use crate::{agents::mcp_scopes, review::Error};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn servers(value: &Value, alias: &str) -> Result<(), Error> {
    if let Some(servers) = value.get("mcpServers") {
        let servers = servers
            .as_object()
            .ok_or(Error::Invalid("invalid Claude MCP scope"))?;
        mcp_scopes::no_alias(alias, servers.keys().map(String::as_str))?;
    }
    Ok(())
}

fn document(path: &Path) -> Result<Option<Value>, Error> {
    mcp_scopes::read(path)?
        .map(|source| {
            let value = crate::json::parse(source.as_bytes())
                .map_err(|_| Error::Invalid("invalid Claude MCP scope JSON"))?;
            if !value.is_object() {
                return Err(Error::Invalid("invalid Claude MCP scope object"));
            }
            Ok(value)
        })
        .transpose()
}

pub(super) fn check(root: &Path, cwd: &Path, alias: &str) -> Result<(), Error> {
    let layers = mcp_scopes::project_layers(root, cwd)?;
    let path = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        Some(directory) => PathBuf::from(directory).join(".claude.json"),
        None => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".claude.json"))
            .ok_or(Error::Invalid("cannot resolve Claude MCP scope"))?,
    };
    if !path.is_absolute() {
        return Err(Error::Invalid("Claude MCP scope must be absolute"));
    }
    if let Some(value) = document(&path)? {
        // User scope is normally below project scope, but can win in Desktop.
        // Refuse duplicates consistently rather than guessing the host surface.
        servers(&value, alias)?;
        if let Some(projects) = value.get("projects") {
            let projects = projects
                .as_object()
                .ok_or(Error::Invalid("invalid Claude project scopes"))?;
            for (project, settings) in projects {
                if layers.iter().any(|layer| layer == Path::new(project))
                    || Path::new(project)
                        .canonicalize()
                        .is_ok_and(|project| layers.contains(&project))
                {
                    servers(settings, alias)?;
                }
            }
        }
    }
    for directory in layers.iter().skip(1) {
        if let Some(value) = document(&directory.join(".mcp.json"))? {
            servers(&value, alias)?;
        }
    }
    Ok(())
}
