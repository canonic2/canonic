//! Request-time connection matching over retained integration predicates.
use super::Plan;
use crate::{
    mcp_connections::{Connection, Connections},
    policy::{Error, selectors},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) struct ResolvedMcp {
    pub(crate) revision: String,
    pub(crate) project_id: Option<String>,
    pub(crate) server: String,
    pub(crate) connection: Option<Connection>,
    pub(crate) source: String,
    /// Private exact-approval binding; never public match provenance.
    pub(crate) connection_digest: Option<String>,
    pub(crate) matches: Vec<McpMatch>,
    pub(super) bindings: Vec<BTreeMap<String, Vec<String>>>,
}

#[derive(Clone, Serialize)]
pub(crate) struct McpMatch {
    pack: String,
    version: String,
    profile: crate::policy::Profile,
    integration: String,
}

impl ResolvedMcp {
    pub(crate) fn facts(&self, arguments: &Value) -> Option<Value> {
        let mut facts = self.connection.as_ref()?.facts(arguments);
        if self.source == "project" {
            facts["id"] = json!(self.server);
        } else if self.source.starts_with("host-") {
            facts["source"] = json!(self.source);
            for evidence in facts["evidence_sources"].as_object_mut()?.values_mut() {
                if evidence["source"] == "reviewed-connection" {
                    evidence["source"] = json!("observed-connection");
                }
            }
        }
        Some(facts)
    }
}

impl Plan {
    pub(crate) fn has_mcp_integrations(&self) -> bool {
        self.metadata
            .packs
            .iter()
            .any(|p| !p.document.mcp_integrations.is_empty())
    }

    pub(crate) fn resolve_mcp(
        &self,
        server: &str,
        connections: &Connections,
        source: &str,
        revision: &str,
    ) -> Result<ResolvedMcp, Error> {
        if connections.len() > 128 {
            return Err(Error::Limit);
        }
        let mut resolved = ResolvedMcp {
            revision: revision.into(),
            project_id: None,
            server: server.into(),
            connection: connections
                .get(server)
                .filter(|c| c.enabled && !c.agents.is_empty())
                .cloned(),
            source: source.into(),
            connection_digest: None,
            matches: vec![],
            bindings: vec![BTreeMap::new(); self.metadata.packs.len()],
        };
        // Serialize each observed transport once, independently of pack count.
        let transports = connections
            .iter()
            .filter(|(_, c)| c.enabled && !c.agents.is_empty())
            .map(|(id, c)| {
                Ok((
                    id,
                    c,
                    serde_json::to_value(&c.transport).map_err(|_| Error::Syntax)?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        if let Some((_, _, transport)) = transports.iter().find(|(id, _, _)| id.as_str() == server)
        {
            resolved.connection_digest =
                Some(crate::json::fingerprint(transport).map_err(|_| Error::Syntax)?);
        }
        let memo = crate::policy::rules::patterns::Memo::new(self, &|| Ok(()));
        for (owner, pack) in self.metadata.packs.iter().enumerate() {
            for (name, integration) in &pack.document.mcp_integrations {
                for (id, connection, transport) in &transports {
                    if connection
                        .packs
                        .get(&pack.document.id)
                        .is_some_and(|selected| selected != name)
                        || transport["type"] != integration.transport
                        || selectors::fields_match(&integration.fields, transport, &memo)?.is_some()
                    {
                        continue;
                    }
                    resolved.bindings[owner]
                        .entry(name.clone())
                        .or_default()
                        .push((*id).clone());
                    if id.as_str() != server {
                        continue;
                    }
                    let current = resolved
                        .connection
                        .as_mut()
                        .expect("enabled current connection");
                    merge(&mut current.command_fields, &integration.command_fields)?;
                    merge(&mut current.effective_fields, &integration.effective_fields)?;
                    resolved.matches.push(McpMatch {
                        pack: pack.document.id.clone(),
                        version: pack.document.version.clone(),
                        profile: pack.profile,
                        integration: name.clone(),
                    });
                }
            }
        }
        Ok(resolved)
    }
}

fn merge<T: Clone + PartialEq>(
    target: &mut BTreeMap<String, T>,
    source: &BTreeMap<String, T>,
) -> Result<(), Error> {
    for (name, value) in source {
        if target.get(name).is_some_and(|existing| existing != value) {
            return Err(Error::Contract(
                "conflicting automatically matched MCP field mappings",
            ));
        }
        target.insert(name.clone(), value.clone());
    }
    Ok(())
}
