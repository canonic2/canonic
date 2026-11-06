//! MCP form confirmation; absent capability keeps proposals pending.
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
    instructions: "Copy apply_arguments unchanged to policy_apply and wait for its MCP confirmation form. Ordinary tool permission does not activate policy. If this client cannot show forms, leave the proposal pending for shield policy review in the person's terminal. ",
    unavailable: "MCP confirmation form unavailable. Leave the proposal pending and ask the person to activate it with shield policy review in their terminal.",
    required: "Approval required. Inspect the proposal with policy_show, copy apply_arguments unchanged to policy_apply, and wait for the MCP confirmation form. If forms are unavailable, leave it pending for terminal review.",
    proposed: "Policy proposal saved; inactive. Copy apply_arguments unchanged to policy_apply for MCP form confirmation. If the client cannot show the form, the person must use shield policy review in their terminal. Decline or cancellation ends the offer; do not retry.",
    ask: "Prepare eligible reviewed groups together with policy_propose. Copy apply_arguments unchanged to policy_apply for MCP form confirmation. Decline or cancellation ends the offer; do not retry. If forms are unavailable, leave the proposal pending for terminal review.",
};
