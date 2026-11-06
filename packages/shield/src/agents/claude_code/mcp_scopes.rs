//! Claude local/user aliases must not lend project rules to another server.
use crate::{agents::mcp_scopes, review::Error};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Approvals {
    all: bool,
    enabled: BTreeSet<String>,
}

impl Approvals {
    fn read(&mut self, value: &Value) -> Result<(), Error> {
        if let Some(all) = value.get("enableAllProjectMcpServers") {
            self.all = all
                .as_bool()
                .ok_or(Error::Invalid("invalid MCP approval setting"))?;
        }
        if let Some(enabled) = value.get("enabledMcpjsonServers") {
            for alias in enabled
                .as_array()
                .ok_or(Error::Invalid("invalid MCP approval list"))?
            {
                self.enabled.insert(
                    alias
                        .as_str()
                        .ok_or(Error::Invalid("invalid MCP approval alias"))?
                        .into(),
                );
            }
        }
        Ok(())
    }
}

fn disabled_servers(value: &Value, disabled: &mut BTreeSet<String>) -> Result<(), Error> {
    for key in ["disabledMcpjsonServers", "disabledMcpServers"] {
        if let Some(aliases) = value.get(key) {
            for alias in aliases
                .as_array()
                .ok_or(Error::Invalid("invalid disabled MCP server list"))?
            {
                disabled.insert(
                    alias
                        .as_str()
                        .ok_or(Error::Invalid("invalid disabled MCP server alias"))?
                        .into(),
                );
            }
        }
    }
    Ok(())
}

fn add_servers(
    observed: &mut mcp_scopes::Observation,
    value: &Value,
    scope: &'static str,
) -> Result<(), Error> {
    if let Some(servers) = value.get("mcpServers") {
        let servers = servers
            .as_object()
            .ok_or(Error::Invalid("invalid Claude MCP scope"))?;
        for (alias, entry) in servers {
            observed.add(alias, super::connections::observe(entry), scope)?;
        }
    }
    Ok(())
}

fn observed_document(
    observed: &mut mcp_scopes::Observation,
    path: &Path,
) -> Result<Option<Value>, Error> {
    observed
        .read(path)?
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

pub(super) fn discover(root: &Path, cwd: &Path) -> Result<mcp_scopes::Observation, Error> {
    let mut observed = mcp_scopes::Observation::new("claude", root, cwd);
    let mut disabled = BTreeSet::new();
    let root = root.canonicalize()?;
    // MCP project/local scope stays at the session project, not the current
    // tool cwd. Validate containment without treating descendants as layers.
    mcp_scopes::project_layers(&root, cwd)?;
    if let Some(host_root) = std::env::var_os("CLAUDE_PROJECT_DIR")
        && Path::new(&host_root).canonicalize()? != root
    {
        return Err(Error::Invalid(
            "Claude session project differs from the enrolled project",
        ));
    }
    let mut approvals = Approvals::default();
    let mut trusted = false;
    let home = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        Some(directory) => PathBuf::from(directory),
        None => std::env::home_dir().ok_or(Error::Invalid("cannot resolve Claude MCP scope"))?,
    };
    if !home.is_absolute() {
        return Err(Error::Invalid("Claude MCP scope must be absolute"));
    }
    if let Some(value) = observed_document(&mut observed, &home.join(".claude.json"))? {
        add_servers(&mut observed, &value, "host-user")?;
        if let Some(projects) = value.get("projects") {
            let projects = projects
                .as_object()
                .ok_or(Error::Invalid("invalid Claude project scopes"))?;
            if let Some(settings) = root.to_str().and_then(|root| projects.get(root)) {
                add_servers(&mut observed, settings, "host-project")?;
                disabled_servers(settings, &mut disabled)?;
                trusted |= settings
                    .get("hasTrustDialogAccepted")
                    .and_then(Value::as_bool)
                    == Some(true);
            }
        }
    }
    let settings = [
        (
            mcp_scopes::configuration("CLAUDE_CONFIG_DIR", ".claude")?.join("settings.json"),
            true,
        ),
        (root.join(".claude/settings.json"), trusted),
        (root.join(".claude/settings.local.json"), trusted),
    ];
    for (path, trusted) in settings {
        if let Some(value) = observed_document(&mut observed, &path)? {
            disabled_servers(&value, &mut disabled)?;
            if trusted {
                approvals.read(&value)?;
            }
        }
    }
    if (approvals.all || !approvals.enabled.is_empty())
        && let Some(value) = observed_document(&mut observed, &root.join(".mcp.json"))?
        && let Some(servers) = value.get("mcpServers")
    {
        for (alias, entry) in servers
            .as_object()
            .ok_or(Error::Invalid("invalid Claude MCP scope"))?
        {
            if approvals.all || approvals.enabled.contains(alias) {
                observed.add(alias, super::connections::observe(entry), "host-project")?;
            }
        }
    }
    for alias in disabled {
        observed.disable(&alias);
    }
    Ok(observed)
}

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
        None => std::env::home_dir()
            .map(|home| home.join(".claude.json"))
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
