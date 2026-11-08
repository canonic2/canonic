//! Agent-owned MCP presentation and confirmation capabilities. Unknown hosts
//! receive inspection/proposal guidance and an explicit in-session capability gap.
use serde_json::Value;

mod form;
mod generic;
pub(super) static FORM: form::Form = form::Form;

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

/// The review adapter of a registered agent ID. No other name inherits a
/// supported agent's confirmation capability.
pub(crate) fn resolve(name: Option<&str>) -> &'static dyn ReviewAdapter {
    name.and_then(super::find)
        .map_or(&generic::Generic, |adapter| adapter.review())
}

/// Bootstrap consent authorizes this connection's form workflow without
/// inferring a native adapter or accepting ordinary tool permission as consent.
pub(crate) fn for_context(context: &crate::review::Context) -> &'static dyn ReviewAdapter {
    if context.surface == "mcp" && context.confirmed_mcp_forms {
        &FORM
    } else {
        resolve(context.agent.as_deref())
    }
}
