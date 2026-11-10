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
    let mut context = super::open_connection(&input.project)?;
    // A declared agent selects its review adapter only when protected project
    // state records its integration; otherwise it is attribution alone.
    let agent = input.agent.as_deref().map(|agent| match agent {
        // Historical Claude MCP entries used this label. Enrollment remains
        // the authority for adapter selection, including for this spelling.
        "claude-code" => "claude",
        other => other,
    });
    if let Some(agent) = agent.filter(|agent| recorded(&context, agent)) {
        context.agent = Some(agent.into());
    } else {
        context.declared_agent.clone_from(&input.agent);
    }
    context.session_id.clone_from(&input.session_id);
    context.tool_use_id.clone_from(&input.tool_use_id);
    Ok(context)
}

/// Whether the project's integration record includes this agent ID,
/// directly or through the recorded Box
/// definition, which launches the server for each adapter with its ID.
/// The checkpoint is read without the writer lock:
/// state is replaced by an atomic rename, so a publication in progress cannot
/// make a connection start without its adapter. State that cannot be read
/// records nothing; the generic adapter then needs terminal activation, and
/// the server's tools report the state problem.
fn recorded(context: &Context, agent: &str) -> bool {
    let Some(adapter) = crate::agents::find(agent) else {
        return false;
    };
    super::storage::Store::inspect(Path::new(&context.project_root))
        .and_then(|store| store.read_diagnostic_checkpoint().map(|(state, _)| state))
        .and_then(|state| {
            state.check(context)?;
            super::integrations::registration(&state.operator)
        })
        .ok()
        .flatten()
        .is_some_and(|registration| registration.covers(adapter.spec().id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_mcp_attribution_selects_only_an_enrolled_adapter() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        super::super::mcp::initialize_fixture(root.path(), &bundle);
        let input = cli::ReviewContext {
            project: root.path().into(),
            agent: Some("claude-code".into()),
            session_id: Some("acme-session".into()),
            tool_use_id: None,
        };
        let before = context(&input).unwrap();
        assert!(before.agent.is_none());
        assert_eq!(before.declared_agent.as_deref(), Some("claude-code"));
        super::super::operator::integrations::configure_fixture(
            root.path(),
            super::super::integrations::IntegrationChoice::agent("claude"),
            super::super::integrations::AskFallback::Block,
        );
        let state =
            super::super::project::checkpoint(&super::super::open(root.path()).unwrap()).unwrap();
        assert!(
            state.operator["integration"]["ask_fallback"]
                .get("codex")
                .is_none()
        );
        let enrolled = context(&input).unwrap();
        assert_eq!(enrolled.agent.as_deref(), Some("claude"));
        assert_eq!(enrolled.session_id, input.session_id);
        assert!(enrolled.declared_agent.is_none());
    }
}
