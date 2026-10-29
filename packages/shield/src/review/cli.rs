//! Registered caller context and bounded host attribution.
use super::{Context, Error};
use crate::cli;
use std::path::Path;

pub(crate) fn context(input: &cli::ReviewContext) -> Result<Context, Error> {
    for field in [&input.agent, &input.session_id, &input.tool_use_id] {
        if field
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 4096 || s.contains('\0'))
        {
            return Err(Error::Invalid("invalid attribution"));
        }
    }
    let mut context = super::open(&input.project)?;
    // A declared agent selects its review adapter only when protected project
    // state records its integration; otherwise it is attribution alone.
    if input
        .agent
        .as_deref()
        .is_some_and(|agent| recorded(&context, agent))
    {
        context.agent.clone_from(&input.agent);
    } else {
        context.declared_agent.clone_from(&input.agent);
    }
    context.session_id.clone_from(&input.session_id);
    context.tool_use_id.clone_from(&input.tool_use_id);
    Ok(context)
}

/// Whether the project's integration record includes this agent, named by
/// adapter or history identity. The checkpoint is read without the writer lock:
/// state is replaced by an atomic rename, so a publication in progress cannot
/// make a connection start without its adapter. State that cannot be read
/// records nothing; the generic adapter then needs terminal activation, and
/// the server's tools report the state problem.
fn recorded(context: &Context, agent: &str) -> bool {
    let Some(adapter) = crate::agents::ADAPTERS
        .iter()
        .find(|a| a.spec().id == agent || a.spec().history_agent == agent)
    else {
        return false;
    };
    super::storage::Store::open(Path::new(&context.project_root))
        .and_then(|store| store.read_checkpoint())
        .and_then(|state| {
            state.check(context)?;
            super::integrations::registration(&state.operator)
        })
        .ok()
        .flatten()
        .is_some_and(|registration| registration.agents.contains_key(adapter.spec().id))
}
