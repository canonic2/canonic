//! Pure resolution of connection defaults, provenance and contributed mappings.
use super::{Error, Pack, Selection};
use crate::mcp_connections::Connections;

pub(super) fn resolve(
    source: &Connections,
    selections: &[Selection],
    captured: &[Pack],
) -> Result<Connections, Error> {
    let mut mcp = source.clone();
    for connection in mcp.values_mut() {
        if let crate::mcp_connections::Transport::Defaults { pack, integration } =
            &connection.transport
        {
            let index = captured
                .iter()
                .position(|p| p.id() == pack)
                .ok_or(Error::Contract("MCP defaults name an unselected pack"))?;
            if connection.packs.get(pack) != Some(integration) {
                return Err(Error::Contract(
                    "MCP defaults require the matching pack association",
                ));
            }
            let variables = captured[index].mcp_default_variables(integration)?;
            let selection = &selections[index];
            connection.local |= selection.layer.is_some()
                || selection.local.as_ref().is_some_and(|l| {
                    variables
                        .iter()
                        .any(|v| l.variables.contains_key(v) || l.unset.contains(v))
                });
            connection.transport = captured[index].mcp_default(integration)?;
        }
    }
    crate::mcp_connections::validate(&mcp)?;
    for connection in mcp.values_mut() {
        for (pack, integration) in &connection.packs {
            let pack = captured
                .iter()
                .find(|p| p.id() == pack)
                .ok_or(Error::Contract("MCP association names an unselected pack"))?;
            for (tool, mapping) in pack.mcp_commands(integration)? {
                if connection
                    .command_fields
                    .get(tool)
                    .is_some_and(|existing| existing != mapping)
                {
                    return Err(Error::Contract("conflicting MCP command field mappings"));
                }
                connection
                    .command_fields
                    .insert(tool.clone(), mapping.clone());
            }
            for (name, mapping) in pack.mcp_effective_fields(integration)? {
                if connection
                    .effective_fields
                    .get(name)
                    .is_some_and(|existing| existing != mapping)
                {
                    return Err(Error::Contract("conflicting effective MCP field mappings"));
                }
                connection
                    .effective_fields
                    .insert(name.clone(), mapping.clone());
            }
        }
    }
    Ok(mcp)
}
