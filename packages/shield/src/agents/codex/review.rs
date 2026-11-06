//! Readable arguments for Codex's per-call MCP tool permission prompt.
use crate::agents::review::{Confirmation, Guidance, ReviewAdapter};
use serde_json::{Value, json};

fn reference(proposal_id: &str) -> String {
    // A bounded selector, not an approval credential. The core regenerates the
    // selected proposal's exact arguments and checks the current revision before
    // committing, which binds its candidate digest, changes and review.
    crate::policy::fingerprint(format!("codex-apply-reference\n{proposal_id}").as_bytes())[..24]
        .to_owned()
}

impl ReviewAdapter for super::Codex {
    fn confirmation(&self) -> Confirmation {
        Confirmation::ToolPermission
    }
    fn guidance(&self) -> &'static Guidance {
        &GUIDANCE
    }
    fn readable_arguments(&self) -> bool {
        true
    }
    fn apply_arguments(&self, canonical: &Value, message: &str) -> Value {
        // Codex's prompt supplies the question and Allow/Cancel choices, then
        // shows the first three arguments by name, each cut to one short line.
        // `change` leads with the first change; `details` holds the rest.
        let (change, details) = message.split_once('\n').unwrap_or((message, ""));
        let mut arguments = json!({
            "change": change,
            "reference": reference(canonical["proposal_id"].as_str().unwrap_or_default())
        });
        let details = details.trim();
        if !details.is_empty() {
            arguments["details"] = json!(details);
        }
        arguments
    }
    fn identifies(&self, arguments: &Value, proposal_id: &str) -> bool {
        arguments["reference"] == reference(proposal_id)
    }
    fn apply_schema(&self) -> Option<Value> {
        Some(json!({"type":"object","additionalProperties":false,
        "required":["change","reference"],"properties":{
            "change":{"type":"string","minLength":1,"maxLength":65536},
            "details":{"type":"string","minLength":1,"maxLength":65536},
            "reference":{"type":"string","minLength":24,"maxLength":24}
        }}))
    }
}

static GUIDANCE: Guidance = Guidance {
    instructions: "Immediately copy apply_arguments unchanged to policy_apply for Codex approval. Connection changes require Shield's form. Closed attempts cannot resume. If approvals are disabled, ask the user to enable prompts with /permissions in this session, then prepare fresh. Never send the user to a terminal or change approval settings yourself. ",
    unavailable: "Codex approval is unavailable and this attempt is closed. Ask the user to enable confirmation prompts with /permissions in this session, then prepare fresh. Do not change approval settings yourself or send the user to a terminal.",
    required: "Approval required. Inspect the live proposal with policy_show, then copy apply_arguments unchanged into policy_apply. Closed attempts require fresh preparation. Do not change approval settings yourself.",
    proposed: "Inactive change prepared. Immediately copy apply_arguments unchanged to policy_apply for Codex approval. Wait for its outcome before preparing another change. Never finish with an undecided proposal; withdraw an abandoned attempt. Save intent with policy_defer only when the user explicitly requests deferral.",
    ask: "For eligible reviewed groups, prepare one bounded batch with policy_propose. Copy apply_arguments unchanged to policy_apply for host approval. Do not repeat unchanged proposed/deferred offers. Decline or cancellation ends the offer; do not retry.",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_selects_one_proposal_by_id_without_exposing_it() {
        let adapter = super::super::Codex;
        let canonical =
            json!({"proposal_id":"acme", "candidate_digest":"one", "changes":[], "review":{}});
        let args = adapter.apply_arguments(&canonical, "Same readable summary");
        let selector = args["reference"].as_str().unwrap();
        assert_eq!(selector.len(), 24);
        assert!(!selector.contains("acme"));
        assert!(adapter.identifies(&args, "acme"));
        assert!(!adapter.identifies(&args, "acme-other"));
        let mut other = canonical.clone();
        other["proposal_id"] = json!("acme-other");
        assert_ne!(
            args["reference"],
            adapter.apply_arguments(&other, "Same readable summary")["reference"]
        );
    }
}
