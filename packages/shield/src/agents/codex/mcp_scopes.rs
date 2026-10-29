//! Refuse ambiguous inherited or closer Codex MCP tables, without merging them.
use crate::{agents::mcp_scopes, review::Error};
use std::path::Path;

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
