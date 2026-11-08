//! Connection projection publication belongs to the same confirmed policy activation.
use super::{Context, Error, Plan, Value, agents, files, json, pending, registration, setup};
use crate::{mcp_connections::Connections, review::storage::Store};
use std::path::Path;
#[cfg(test)]
mod tests;

pub(in crate::review) fn prepare(
    root: &Path,
    metadata: &Value,
    before: &Connections,
    after: &Connections,
    revision: &str,
) -> Result<Option<Plan>, Error> {
    let Some(previous) = registration(metadata)? else {
        return Ok(None);
    };
    let targets: Vec<String> = agents::ADAPTERS
        .iter()
        .map(|a| a.spec().id)
        .filter(|agent| previous.covers(agent) && affected(before, after, agent))
        .map(str::to_owned)
        .collect();
    if targets.is_empty() {
        return Ok(None);
    }
    if pending(metadata)?.is_some() {
        return Err(Error::Invalid(
            "confirmed agent setup is pending; continue or review it in this session before changing connections",
        ));
    }
    let shared = targets
        .iter()
        .any(|id| agents::find(id).is_some_and(|a| a.spec().mcp_path.is_none()));
    let before = if shared {
        super::read_file(root)?
    } else {
        None
    };
    let mut desired = previous.clone();
    let mut files = Vec::new();
    let after = setup::prepare_configured(
        root,
        before.as_deref().unwrap_or(""),
        Some(&previous),
        Some(&mut desired),
        &mut files,
        &targets,
        after,
    )?;
    let plan = Plan {
        targets: Some(targets),
        scope_reason: None,
        policy_revision: Some(revision.into()),
        before,
        after,
        previous: Some(previous),
        desired: Some(desired),
        files,
        handed_over: Vec::new(),
    };
    super::maintenance::validate(&plan)?;
    super::privacy::validate(root, &plan)?;
    files::validate_plan_before(root, &plan)?;
    Ok(Some(plan))
}

fn affected(before: &Connections, after: &Connections, agent: &str) -> bool {
    before.keys().chain(after.keys()).any(|id| {
        let old = before
            .get(id)
            .filter(|connection| connection.agents.contains_key(agent));
        let new = after
            .get(id)
            .filter(|connection| connection.agents.contains_key(agent));
        match (old, new) {
            (Some(old), Some(new)) => {
                old.enabled != new.enabled
                    || old.local != new.local
                    || old.transport != new.transport
                    || old.agents.get(agent) != new.agents.get(agent)
                    || old.packs != new.packs
            }
            (None, None) => false,
            _ => true,
        }
    })
}

pub(in crate::review) fn stage(
    metadata: &mut Value,
    plan: &Plan,
    proposal: Option<&str>,
) -> Result<(), Error> {
    metadata["integration_pending"] = json!(plan);
    metadata["integration_publication"] = json!({"proposal_id":proposal,"plan_digest":crate::json::fingerprint(plan).map_err(|_|Error::Invalid("connection publication encoding"))?});
    Ok(())
}

pub(in crate::review) fn bind(state: &mut super::State, metadata: &mut Value, receipt: &Value) {
    if metadata["integration_publication"].is_object()
        && metadata["integration_publication"]
            .get("activation_id")
            .is_none()
    {
        metadata["integration_publication"]["activation_id"] = receipt["activation_id"].clone();
        state.operator["integration_publication"] = metadata["integration_publication"].clone();
    }
}

/// Retry only this activation's exact confirmed plan, never a later setup.
pub(in crate::review) fn resume(
    context: &Context,
    proposal: Option<&str>,
    activation: &str,
) -> Result<bool, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    if state
        .active
        .as_ref()
        .is_none_or(|active| active.activation_id() != activation)
    {
        return Ok(false);
    }
    let Some(plan) = pending(&state.operator)? else {
        return Ok(true);
    };
    let marker = &state.operator["integration_publication"];
    if !marker.is_object()
        || marker["activation_id"] != activation
        || marker["proposal_id"] != json!(proposal)
        || marker["plan_digest"]
            != crate::json::fingerprint(&plan)
                .map_err(|_| Error::Invalid("connection publication encoding"))?
    {
        return Ok(false);
    }
    setup::resume_plan(context, &state.operator, &plan, Some(activation))?;
    Ok(true)
}

/// Check under each publication writer, including after partial publication.
pub(super) fn check_owner(
    state: &super::State,
    expected: &Value,
    activation: Option<&str>,
) -> Result<(), Error> {
    if activation.is_some_and(|id| {
        state
            .active
            .as_ref()
            .is_none_or(|active| active.activation_id() != id)
            || state.operator["integration_publication"] != expected["integration_publication"]
    }) {
        return Err(Error::StaleSnapshot);
    }
    Ok(())
}

/// A cached launcher can finish its own confirmed publication before resolving
/// its locator. Unknown locators and unrelated setup cause no host writes.
pub(crate) fn resume_launch(context: &Context, token: &str) -> Result<(), Error> {
    if !crate::mcp_connections::launcher::valid_token(token) {
        return Ok(());
    }
    crate::mcp_connections::launcher::private_state(Path::new(&context.project_root))?;
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    let marker = &state.operator["integration_publication"];
    if !marker.is_object() {
        return Ok(());
    }
    let Some(plan) = pending(&state.operator)? else {
        return Ok(());
    };
    let owns = [plan.previous.as_ref(), plan.desired.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|registration| registration.connections.iter())
        .filter(|(agent, _)| plan.affects(agent))
        .any(|(agent, owned)| {
            owned["launches"]
                .as_object()
                .into_iter()
                .flatten()
                .any(|(id, binding)| {
                    let old = plan
                        .previous
                        .as_ref()
                        .and_then(|r| r.connections.get(agent))
                        .and_then(|owned| owned["launches"].get(id));
                    let new = plan
                        .desired
                        .as_ref()
                        .and_then(|r| r.connections.get(agent))
                        .and_then(|owned| owned["launches"].get(id));
                    binding["token"] == token && old != new
                })
        });
    if owns && let Some(active) = state.active.as_ref() {
        resume(
            context,
            marker["proposal_id"].as_str(),
            active.activation_id(),
        )?;
    }
    Ok(())
}
