//! Agent-owned MCP presentation and confirmation capabilities. Unknown hosts
//! receive inspection/proposal guidance and must activate policy in a terminal.
use serde_json::Value;

mod generic;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confirmation {
    Terminal,
    Form,
    ToolPermission,
}

pub(crate) struct Guidance {
    pub instructions: &'static str,
    pub unavailable: &'static str,
    pub required: &'static str,
    pub proposed: &'static str,
    /// How to ask the user which rule suggestions to allow.
    pub ask: &'static str,
}

/// Encodes host presentation only. The shared review core validates retained
/// proposals, freshness, exact arguments and confirmation before activation.
pub(crate) trait ReviewAdapter: Sync {
    fn confirmation(&self) -> Confirmation;
    fn guidance(&self) -> &'static Guidance;
    fn readable_arguments(&self) -> bool {
        false
    }
    fn apply_arguments(&self, canonical: &Value, _message: &str) -> Value {
        canonical.clone()
    }
    /// Selects a retained proposal from its ID alone, without rendering it.
    fn identifies(&self, arguments: &Value, proposal_id: &str) -> bool {
        arguments["proposal_id"] == proposal_id
    }
    fn apply_schema(&self) -> Option<Value> {
        None
    }
}

/// Registered/native agent names and history identities share one resolver.
/// No unknown name inherits a supported agent's confirmation capability.
pub(crate) fn resolve(name: Option<&str>) -> &'static dyn ReviewAdapter {
    super::ADAPTERS
        .iter()
        .find(|adapter| {
            name.is_some_and(|name| {
                name == adapter.spec().id || name == adapter.spec().history_agent
            })
        })
        .map_or(&generic::Generic, |adapter| adapter.review())
}
