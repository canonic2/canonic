//! Refuse ambiguous inherited or closer Codex MCP tables, without merging them.
use crate::{agents::mcp_scopes, review::Error};
use std::path::Path;

fn add_servers(
    observed: &mut mcp_scopes::Observation,
    doc: &toml_edit::DocumentMut,
    scope: &'static str,
) -> Result<(), Error> {
    if let Some(servers) = doc.get("mcp_servers") {
        let servers = servers
            .as_table_like()
            .ok_or(Error::Invalid("invalid Codex MCP scope table"))?;
        for (alias, entry) in servers.iter() {
            observed.add(alias, super::setup::observe_connection(entry), scope)?;
        }
    }
    Ok(())
}

fn document(
    observed: &mut mcp_scopes::Observation,
    path: &Path,
) -> Result<Option<toml_edit::DocumentMut>, Error> {
    observed
        .read(path)?
        .map(|source| {
            source
                .parse()
                .map_err(|_| Error::Invalid("invalid Codex MCP scope TOML"))
        })
        .transpose()
}

pub(super) fn discover(root: &Path, cwd: &Path) -> Result<mcp_scopes::Observation, Error> {
    let mut observed = mcp_scopes::Observation::new("codex", root, cwd);
    let layers = mcp_scopes::project_layers(root, cwd)?;
    let user = mcp_scopes::configuration("CODEX_HOME", ".codex")?.join("config.toml");
    let mut trusted = false;
    if let Some(doc) = document(&mut observed, &user)? {
        add_servers(&mut observed, &doc, "host-user")?;
        if let Some(projects) = doc.get("projects") {
            let projects = projects
                .as_table_like()
                .ok_or(Error::Invalid("invalid Codex project trust table"))?;
            let trust = |path: &Path| projects.get(path.to_str()?)?.get("trust_level")?.as_str();
            trusted = trust(&layers[0]) == Some("trusted")
                && !layers.iter().any(|path| trust(path) == Some("untrusted"));
        }
    }
    if trusted {
        for directory in layers {
            if let Some(doc) = document(&mut observed, &directory.join(".codex/config.toml"))? {
                add_servers(&mut observed, &doc, "host-project")?;
            }
        }
    }
    Ok(observed)
}

fn check_file(path: &Path, alias: &str) -> Result<(), Error> {
    if let Some(source) = mcp_scopes::read(path)? {
        let document: toml_edit::DocumentMut = source
            .parse()
            .map_err(|_| Error::Invalid("invalid Codex MCP scope TOML"))?;
        if let Some(servers) = document.get("mcp_servers") {
            let servers = servers
                .as_table_like()
                .ok_or(Error::Invalid("invalid Codex MCP scope table"))?;
            mcp_scopes::no_alias(alias, servers.iter().map(|(name, _)| name))?;
        }
    }
    Ok(())
}

pub(super) fn check(root: &Path, cwd: &Path, alias: &str) -> Result<(), Error> {
    check_file(
        &mcp_scopes::configuration("CODEX_HOME", ".codex")?.join("config.toml"),
        alias,
    )?;
    for directory in mcp_scopes::project_layers(root, cwd)?.iter().skip(1) {
        check_file(&directory.join(".codex/config.toml"), alias)?;
    }
    Ok(())
}
