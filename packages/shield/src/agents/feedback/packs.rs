//! Advisory pack offers shared by native agents and approval follow-ups.
use crate::{agents::AgentAdapter, proposal_draft, review::observations};
use serde_json::Value;
use std::path::Path;

pub(super) fn offer(
    root: &Path,
    adapter: &dyn AgentAdapter,
    guidance: &Value,
    entry: Option<&str>,
) -> Option<String> {
    // This request is the analyzed decisive command, never the submitted shell
    // string. Analyzer refusals do not have a fix in a permission pack.
    if guidance["source"] != "decisive" {
        return None;
    }
    let executable = guidance["request"]["executable"].as_str()?;
    let references = observations::uninstalled_packs(root, executable);
    match references.as_slice() {
        [] => None,
        [reference] => {
            let name = reference.rsplit('/').next().unwrap_or(reference);
            let instruction = adapter
                .pack_offer_instruction()
                .map_or_else(String::new, |text| format!("{text}\n"));
            Some(format!(
                "{instruction}The `{name}` rule pack advertises this command; profiles may still ask or deny it. Inspect pack_info for {reference} before offering it: describe examples of Allow and retained Ask/Deny outcomes for the proposed profile, including conditional outcomes. Do not promise that installing a pack removes prompts for every advertised command. Use this short name with the user and the full reference in tools. {}",
                proposal_draft::offer(&proposal_draft::arguments(
                    &proposal_draft::pack(reference),
                    entry
                ))
            ))
        }
        many => {
            let shown = many
                .iter()
                .take(3)
                .map(|reference| format!("`{reference}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let more = if many.len() > 3 {
                format!(" ({} more candidates)", many.len() - 3)
            } else {
                String::new()
            };
            Some(format!(
                "Several available packs advertise this command: {shown}{more}. Inspect with pack_info and choose with the user; relevance grants no permission. Use short names unless ambiguous; keep full references in tools. Call policy_propose with change {{\"kind\":\"pack_add\",\"reference\":\"<selected full reference>\"}} and agent_reason, then copy the returned apply_arguments object unchanged into policy_apply for approval."
            ))
        }
    }
}
