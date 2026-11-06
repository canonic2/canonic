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
    instructions: "Immediately copy apply_arguments unchanged to policy_apply. Wait for its MCP confirmation form in default, auto and bypassPermissions modes; ordinary tool approval does not activate policy. Withdraw abandoned attempts before finishing. Use policy_defer only for explicitly requested deferred intent. ",
    unavailable: "MCP confirmation unavailable and this attempt is closed. Ask the user to allow Shield's confirmation prompts in this session, then prepare fresh. Do not send the user to a terminal.",
    required: "Approval required. Inspect the proposal with policy_show using its proposal_id, then copy its apply_arguments object unchanged into policy_apply.",
    proposed: "Inactive change prepared. Immediately copy apply_arguments unchanged to policy_apply for MCP confirmation. Wait for its outcome before preparing another change; withdraw anything abandoned before finishing.",
    ask: "For eligible reviewed groups, prepare one bounded batch with policy_propose. Copy apply_arguments unchanged to policy_apply for MCP form confirmation. Do not repeat unchanged proposed/deferred offers. Decline or cancellation ends the offer; do not retry.",
};
