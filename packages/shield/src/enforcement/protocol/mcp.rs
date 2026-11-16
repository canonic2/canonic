//! One request's explicit or discovered connection, shared through handoff.
use super::{Prepared, PreparedMcp};
use crate::review::{Error, integrations};
use std::{path::Path, sync::Arc};

impl Prepared {
    pub(crate) fn prepare_mcp(
        &mut self,
        root: &Path,
        cwd: &Path,
        agent: &str,
        alias: &str,
    ) -> Result<bool, Error> {
        let connections = self.snapshot.mcp_connections();
        if crate::agents::reserved_mcp_alias(alias) {
            if connections
                .values()
                .any(|c| c.agents.get(agent).is_some_and(|a| a == alias))
            {
                return Err(Error::Invalid(
                    "configured MCP alias conflicts with a host-owned namespace",
                ));
            }
            // Plugin/app connectors have no supported configuration observation.
            // Never answer their host prompt using a repository's alias claim.
            return Ok(false);
        }
        if let Some((server, connection)) = self.snapshot.mcp_connection(agent, alias) {
            let configured = &connection.agents[agent];
            let transport = integrations::mcp_projection(root, agent, server, connection)?;
            integrations::verify_mcp_connection_at(root, cwd, agent, configured, &transport)?;
            let mut active = connections.clone();
            for connection in active.values_mut() {
                connection.enabled &= connection
                    .agents
                    .get(agent)
                    .is_some_and(|alias| !crate::agents::reserved_mcp_alias(alias));
            }
            let resolved = self
                .snapshot
                .resolve_mcp(server, &active, "project")
                .map_err(|_| Error::Invalid("MCP policy matching failed"))?;
            self.mcp = Some(PreparedMcp {
                namespace: alias.into(),
                alias: configured.into(),
                resolved: Arc::new(resolved),
                observation: None,
            });
            return Ok(true);
        }
        if connections.values().any(|c| {
            c.agents.get(agent).is_some_and(|a| {
                crate::agents::mcp_namespace(agent, a).is_ok_and(|encoded| encoded == alias)
            })
        }) {
            return Err(Error::Invalid("MCP project binding is inactive"));
        }
        if !self.snapshot.has_mcp_integrations() {
            return Ok(false);
        }
        let Some(observation) = crate::agents::find(agent)
            .ok_or(Error::Invalid("unknown MCP agent"))?
            .discover_mcp(root, cwd)?
        else {
            return Ok(false);
        };
        let Some((configured, current)) = observation.resolve(alias)? else {
            return Ok(false);
        };
        let source = current.source;
        let discovered = observation.connections(connections, agent);
        let resolved = self
            .snapshot
            .resolve_mcp(&format!("host:{configured}"), &discovered, source)
            .map_err(|_| {
                Error::Invalid("MCP integration predicates or connector mappings conflict")
            })?;
        if resolved.matches.is_empty() {
            return Ok(false);
        }
        self.mcp = Some(PreparedMcp {
            namespace: alias.into(),
            alias: configured.into(),
            resolved: Arc::new(resolved),
            observation: Some(observation),
        });
        Ok(true)
    }
}
