//! Conservative MCP fallback; it does not implement native hook installation.
use super::{Confirmation, Guidance, ReviewAdapter};

pub(super) struct Generic;
impl ReviewAdapter for Generic {
    fn confirmation(&self) -> Confirmation {
        Confirmation::Terminal
    }
    fn guidance(&self) -> &'static Guidance {
        &GUIDANCE
    }
}

static GUIDANCE: Guidance = Guidance {
    instructions: "If this host cannot confirm changes, the user must review and activate pending proposals with shield policy review in their terminal. ",
    unavailable: "Host confirmation unavailable. Ask the user to activate the pending proposal with shield policy review in their terminal.",
    required: "User action required: review and activate the proposal with shield policy review in a terminal.",
    proposed: "Policy proposal saved; inactive. Ask the user to review and activate it with shield policy review in their terminal.",
    ask: "In your final reply, explain each optional rule with Keep asking and Allow without asking choices. Propose chosen rules together with changes; the user activates them with shield policy review.",
};
