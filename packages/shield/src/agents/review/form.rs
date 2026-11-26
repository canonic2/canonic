//! MCP form confirmation; absent capability closes the attempt.
use super::{Confirmation, Guidance, ReviewAdapter};
pub(in crate::agents) struct Form;
impl ReviewAdapter for Form {
    fn confirmation(&self) -> Confirmation {
        Confirmation::Form
    }
    fn guidance(&self) -> &'static Guidance {
        &GUIDANCE
    }
}
static GUIDANCE: Guidance = Guidance {
    instructions: "Wait for Shield's MCP confirmation form for policy changes.",
    unavailable: "MCP confirmation form unavailable. This attempt is closed; report the missing in-session capability. Never send the user to a terminal.",
    required: "Approval required. Inspect the live proposal with policy_show, copy apply_arguments unchanged to policy_apply, and wait for its MCP confirmation form. Closed attempts require fresh preparation.",
    proposed: "Inactive change prepared. Immediately copy apply_arguments unchanged to policy_apply for MCP form confirmation. Decline, cancellation or unavailable confirmation closes the attempt. Never send the user to a terminal.",
    ask: "Prepare eligible reviewed groups together with policy_propose and immediately copy apply_arguments unchanged to policy_apply. Decline or cancellation ends the offer; do not retry. Unavailable forms close the attempt; report the capability gap.",
};
