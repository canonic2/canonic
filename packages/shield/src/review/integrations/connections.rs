//! Active project connection diagnostics, separate from native hook ownership.
use super::{Path, Registration, Value, agents, json, review, verify_mcp_connection};
use crate::mcp_connections::{Connection, Transport, launcher::Binding};

#[cfg(test)]
mod tests;

/// One committed launcher and its exact owned host entry. Other aliases in the
/// same host file do not participate in this connection's authority.
fn retained_launch<'a>(
    registration: &'a Registration,
    agent: &str,
    id: &str,
) -> Option<(&'a str, &'a Value, &'a Value)> {
    if !registration.covers(agent) {
        return None;
    }
    let owned = registration.connections.get(agent)?;
    let binding = owned["launches"].get(id)?;
    let mut rows = owned["review"]
        .as_array()?
        .iter()
        .filter(|row| row["id"] == id && row["enabled"] == true);
    let alias = rows.next()?["alias"].as_str()?;
    if rows.next().is_some() {
        return None;
    }
    let entry = owned["entries"]
        .get(alias)
        .filter(|entry| !entry.is_null())?;
    Some((alias, binding, entry))
}

fn validate_pending_launch(
    metadata: &Value,
    registered: &Registration,
    agent: &str,
    id: &str,
) -> Result<(), super::Error> {
    let Some(plan) = super::pending(metadata)? else {
        return Ok(());
    };
    let unchanged = plan
        .previous
        .as_ref()
        .zip(plan.desired.as_ref())
        .is_some_and(|(previous, desired)| {
            registered.binary == previous.binary
                && registered.binary == desired.binary
                && retained_launch(registered, agent, id).is_some_and(|committed| {
                    retained_launch(previous, agent, id) == Some(committed)
                        && retained_launch(desired, agent, id) == Some(committed)
                })
        });
    if unchanged {
        Ok(())
    } else {
        Err(super::Error::Invalid(
            "this private MCP connection has an unsettled setup change",
        ))
    }
}

pub(super) fn native_matches(root: &Path, agent: &str, connection: &Connection) -> bool {
    connection.agents.get(agent).is_some_and(|alias| {
        verify_mcp_connection(root, agent, alias, &connection.transport).is_ok()
    })
}

pub(super) fn projected(
    root: &Path,
    registration: &Registration,
    agent: &str,
    id: &str,
    connection: &Connection,
) -> Result<Transport, super::Error> {
    let canonical_root = root.canonicalize()?;
    let root = canonical_root.as_path();
    // The activated transport is already reviewed authority. An existing native
    // entry that matches it needs no generated launcher or installation refresh.
    // Verification includes competing host scopes; alias equality alone is not
    // evidence of connection identity.
    if connection.local && native_matches(root, agent, connection) {
        return Ok(connection.transport.clone());
    }
    let owned = registration.connections.get(agent).unwrap_or(&Value::Null);
    let binding = if connection.local {
        let binding: Binding =
            serde_json::from_value(owned["launches"][id].clone()).map_err(|_| {
                super::Error::Invalid("private MCP launch binding needs reviewed setup")
            })?;
        if !binding.matches(connection) {
            return Err(super::Error::Invalid("private MCP launch settings changed"));
        }
        Some(binding)
    } else {
        None
    };
    match binding {
        Some(binding)
            if agents::find(agent)
                .is_some_and(crate::agents::AgentAdapter::portable_mcp_launcher) =>
        {
            let portable = binding.portable_projected(connection, &registration.binary, root)?;
            if connection
                .agents
                .get(agent)
                .is_some_and(|alias| verify_mcp_connection(root, agent, alias, &portable).is_ok())
            {
                Ok(portable)
            } else {
                // Exact old owned launchers remain valid until reviewed setup
                // or epoch maintenance replaces their historical path codec.
                binding.projected(connection, &registration.binary, root)
            }
        }
        Some(binding) => binding.projected(connection, &registration.binary, root),
        None => Ok(connection.projected(&registration.binary, root, None)?),
    }
}

pub(crate) fn mcp_projection(
    root: &Path,
    agent: &str,
    id: &str,
    connection: &Connection,
) -> Result<Transport, super::Error> {
    if !connection.local {
        return Ok(connection.transport.clone());
    }
    let state = review::storage::Store::open(root)?.read_checkpoint()?;
    state.check_operations()?;
    if native_matches(root, agent, connection) {
        return Ok(connection.transport.clone());
    }
    crate::mcp_connections::launcher::private_state(root)?;
    let registration = super::registration(&state.operator)?.ok_or(super::Error::Invalid(
        "private MCP launch is not registered",
    ))?;
    validate_pending_launch(&state.operator, &registration, agent, id)?;
    projected(root, &registration, agent, id, connection)
}

pub(crate) fn resolve_mcp_token(
    root: &Path,
    token: &str,
) -> Result<(String, Binding), super::Error> {
    if !crate::mcp_connections::launcher::valid_token(token) {
        return Err(super::Error::Invalid("invalid private MCP token"));
    }
    crate::mcp_connections::launcher::private_state(root)?;
    let state = review::storage::Store::open(root)?.read_checkpoint()?;
    state.check_operations()?;
    let registration = super::registration(&state.operator)?.ok_or(super::Error::Invalid(
        "private MCP launch is not registered",
    ))?;
    for (agent, owned) in &registration.connections {
        for (id, value) in owned["launches"].as_object().into_iter().flatten() {
            if let Ok(binding) = serde_json::from_value::<Binding>(value.clone())
                && binding.token == token
            {
                validate_pending_launch(&state.operator, &registration, agent, id)?;
                return Ok((id.clone(), binding));
            }
        }
    }
    Err(super::Error::Invalid("private MCP token is not registered"))
}

pub(super) fn diagnose(
    root: &Path,
    snapshot: Option<&review::cache::Inspection>,
    registered: Option<&Registration>,
    diagnostics: &mut Vec<&'static str>,
) -> Vec<Value> {
    let mut rows = Vec::new();
    if let Some(snapshot) = snapshot {
        if let Some(registration) = registered {
            for (agent, owned) in &registration.connections {
                for alias in owned["entries"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(alias, _)| alias)
                {
                    if snapshot.mcp_connection(agent, alias).is_none() {
                        diagnostics.push("owned MCP projection is inactive; run reviewed setup");
                    }
                }
            }
        }
        for (id, connection) in snapshot.mcp_connections() {
            for (agent, alias) in &connection.agents {
                let installed = registered.as_ref().is_some_and(|r| r.covers(agent));
                let matches = registered
                    .as_ref()
                    .and_then(|r| projected(root, r, agent, id, connection).ok())
                    .is_some_and(|transport| {
                        verify_mcp_connection(root, agent, alias, &transport).is_ok()
                    });
                let configured = if !connection.enabled {
                    "disabled"
                } else if !installed {
                    "hooks missing"
                } else if matches {
                    "matches reviewed settings"
                } else {
                    "missing or changed"
                };
                if connection.enabled && (!installed || !matches) {
                    diagnostics.push("project MCP binding needs reviewed setup");
                }
                rows.push(json!({"id":id,"agent":agent,"alias":alias,"configuration":configured,"source":if connection.local {"local"}else{"project"},"packs":connection.packs,"credential_readiness":"unverified","host_loading":"unverified","effective_connection_identity":"unverified"}));
            }
        }
        if snapshot.has_mcp_integrations() {
            automatic(root, snapshot, registered, diagnostics, &mut rows);
        }
    }
    // Several aliases can share a repair route; retain connection-specific
    // detail in rows without repeating the same problem in the human report.
    let mut seen = std::collections::BTreeSet::new();
    diagnostics.retain(|diagnostic| seen.insert(*diagnostic));
    rows
}

fn automatic(
    root: &Path,
    snapshot: &review::cache::Inspection,
    registered: Option<&Registration>,
    diagnostics: &mut Vec<&'static str>,
    rows: &mut Vec<Value>,
) {
    for agent in ["claude", "codex"] {
        let adapter = crate::agents::find(agent).expect("supported discovery adapter");
        let Ok(Some(observation)) = adapter.discover_mcp(root, root) else {
            diagnostics.push("automatic MCP discovery unavailable; inspect host configuration");
            rows.push(json!({"agent":agent,"mode":"automatic","configuration":"discovery unavailable","effective_connection_identity":"unverified"}));
            continue;
        };
        let hooks = if registered.is_some_and(|r| r.agents.contains_key(agent)) {
            "registered; host loading unverified"
        } else if registered.is_some_and(|r| r.box_definition.is_some()) {
            "defined for Box; host loading unverified"
        } else {
            "not registered"
        };
        for (alias, entry) in &observation.entries {
            if snapshot
                .mcp_connections()
                .values()
                .any(|c| c.agents.get(agent) == Some(alias))
            {
                continue;
            }
            let resolution = observation.get(alias);
            let configuration = match resolution {
                Ok(_) if entry.transport.as_ref().is_ok_and(Option::is_none) => {
                    "unsupported discovery scope; host permissions apply"
                }
                Ok(_) => "policy matching checked on interception",
                Err(_) => "ambiguous or unsupported configuration",
            };
            rows.push(json!({"agent":agent,"alias":alias,"mode":"automatic","source":entry.source,
                "configuration":configuration,"matches":[],"matching":"unchecked","hooks":hooks,
                "credential_readiness":"unverified","host_loading":"unverified","effective_connection_identity":"unverified"}));
        }
    }
}
