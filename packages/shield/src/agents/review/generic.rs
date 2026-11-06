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
    instructions: "This host has no supported in-session approval adapter. Report that capability gap; never send the user to a terminal for approvals. ",
    unavailable: "This host cannot obtain in-session approval. Report the missing capability; do not ask the user to run terminal commands.",
    required: "In-session approval is unavailable for this host. Report the missing capability and close the attempt.",
    proposed: "Inactive change prepared. This host cannot obtain in-session approval; withdraw the attempt and report the missing capability. Do not send the user to a terminal.",
    ask: "Explain optional rules without creating an undecided backlog. This host cannot obtain in-session approval; report the missing capability.",
};
