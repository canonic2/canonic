//! Active project connection diagnostics, separate from native hook ownership.
use super::{Context, Path, Registration, Value, json, review, verify_mcp_connection};
use crate::mcp_connections::{Connection, Transport, launcher::Binding};

pub(super) fn projected(
    root: &Path,
    registration: &Registration,
    agent: &str,
    id: &str,
    connection: &Connection,
) -> Result<Transport, super::Error> {
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
    Ok(connection.projected(
        &registration.binary,
        root,
        binding.as_ref().map(|binding| binding.token.as_str()),
    )?)
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
    crate::mcp_connections::launcher::private_state(root)?;
    let state = review::project::load(&review::open(root)?)?;
    if super::pending(&state.operator)?.is_some() {
        return Err(super::Error::Invalid("private MCP setup requires repair"));
    }
    let registration = super::registration(&state.operator)?.ok_or(super::Error::Invalid(
        "private MCP launch is not registered",
    ))?;
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
    let state = review::project::load(&review::open(root)?)?;
    if super::pending(&state.operator)?.is_some() {
        return Err(super::Error::Invalid("private MCP setup requires repair"));
    }
    let registration = super::registration(&state.operator)?.ok_or(super::Error::Invalid(
        "private MCP launch is not registered",
    ))?;
    for owned in registration.connections.values() {
        for (id, value) in owned["launches"].as_object().into_iter().flatten() {
            if let Ok(binding) = serde_json::from_value::<Binding>(value.clone())
                && binding.token == token
            {
                return Ok((id.clone(), binding));
            }
        }
    }
    Err(super::Error::Invalid("private MCP token is not registered"))
}

pub(super) fn diagnose(
    context: &Context,
    registered: Option<&Registration>,
    diagnostics: &mut Vec<&'static str>,
) -> Vec<Value> {
    let root = Path::new(&context.project_root);
    let mut rows = Vec::new();
    if let Ok(snapshot) = review::runtime::consumer::load_cached(context, &|| Ok(())) {
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
                let installed = registered
                    .as_ref()
                    .is_some_and(|r| r.agents.contains_key(agent));
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
    }
    rows
}
