//! Claude's existing form confirmation and canonical approval arguments.
use crate::agents::review::{Confirmation, Guidance, ReviewAdapter};

impl ReviewAdapter for super::ClaudeCode {
    fn confirmation(&self) -> Confirmation {
        Confirmation::Form
    }
    fn guidance(&self) -> &'static Guidance {
        &GUIDANCE
    }
}

static GUIDANCE: Guidance = Guidance {
    instructions: "Copy apply_arguments unchanged to policy_apply. Wait for its MCP confirmation form in default, auto and bypassPermissions modes; ordinary tool approval does not activate policy. ",
    unavailable: "MCP confirmation unavailable. Ask the user to enable MCP form elicitation, or activate the pending proposal with shield policy review in their terminal.",
    required: "Approval required. Inspect the proposal with policy_show using its proposal_id, then copy its apply_arguments object unchanged into policy_apply.",
    proposed: "Policy proposal saved; inactive. Copy apply_arguments unchanged to policy_apply for MCP confirmation. Wait before the next proposal; activation stales other proposals.",
    ask: "For eligible reviewed groups, prepare one bounded batch with policy_propose. Copy apply_arguments unchanged to policy_apply for MCP form confirmation. Do not repeat unchanged proposed/deferred offers. Decline or cancellation ends the offer; do not retry.",
};
