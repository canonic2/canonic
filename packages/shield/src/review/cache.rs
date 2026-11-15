//! The per-user cache folder and the coverage facts status compares agents'
//! files with. Compiled plans themselves belong to `files.rs`.
use super::Error;
use crate::{configuration::Evaluator, policy};
use std::path::PathBuf;

/// Nonauthorizing coverage facts of the policy files' plan: Codex prefix
/// rules and MCP connections the agents' files should match.
pub(in crate::review) struct Inspection {
    prefixes: Vec<policy::rules::agent_outputs::CodexPrefix>,
    connections: crate::mcp_connections::Connections,
    automatic_mcp: bool,
}

impl Inspection {
    /// The coverage facts of `evaluator`, the plan the policy files load.
    pub(in crate::review) fn of(evaluator: &Evaluator) -> Self {
        Self {
            prefixes: evaluator.codex_prefixes().to_vec(),
            connections: evaluator.effective().mcp_connections().clone(),
            automatic_mcp: evaluator.has_mcp_integrations(),
        }
    }
    pub(in crate::review) fn codex_prefixes(&self) -> &[policy::rules::agent_outputs::CodexPrefix] {
        &self.prefixes
    }
    pub(in crate::review) fn mcp_connections(&self) -> &crate::mcp_connections::Connections {
        &self.connections
    }
    pub(in crate::review) fn mcp_connection(
        &self,
        agent: &str,
        alias: &str,
    ) -> Option<&crate::mcp_connections::Connection> {
        self.connections.values().find(|connection| {
            connection.enabled
                && connection
                    .agents
                    .get(agent)
                    .is_some_and(|name| name == alias)
        })
    }
    pub(in crate::review) fn has_mcp_integrations(&self) -> bool {
        self.automatic_mcp
    }
}

/// The per-user cache folder: compiled plans and pack artifacts.
pub(super) fn default_directory() -> Result<PathBuf, Error> {
    #[cfg(test)]
    return Ok(std::env::temp_dir()
        .canonicalize()?
        .join(format!("shield-unit-cache-{}", std::process::id())));
    #[cfg(not(test))]
    {
        #[cfg(target_os = "windows")]
        return std::env::var_os("LOCALAPPDATA")
            .map(|home| PathBuf::from(home).join("Shield/Cache"))
            .filter(|p| p.is_absolute())
            .ok_or(Error::Invalid("Local AppData unavailable; use --cache-dir"));
        #[cfg(not(target_os = "windows"))]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or(Error::Invalid(
                    "home directory unavailable; use --cache-dir",
                ))?;
            #[cfg(target_os = "macos")]
            return Ok(home.join("Library/Caches/Shield"));
            #[cfg(not(target_os = "macos"))]
            return Ok(std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".cache"))
                .join("shield"));
        }
    }
}
