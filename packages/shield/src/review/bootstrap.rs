//! Whether a person reviewed this project's agent integration through a host form.
use super::{model::State, storage::Store};
use std::path::Path;

#[cfg(test)]
mod tests;

/// Continue the reviewed host's form capability after reconnecting, moving or
/// copying the folder. This metadata supplies no native agent identity or
/// operation authority.
pub(crate) fn forms_reviewed_at(project: &Path) -> bool {
    // Inspection converts an earlier checkpoint in memory only.
    Store::inspect(project)
        .and_then(|store| store.read_diagnostic_checkpoint())
        .is_ok_and(|(state, _)| integration_forms_reviewed(&state))
}

fn integration_forms_reviewed(state: &State) -> bool {
    let receipt = &state.operator["integration_review"];
    receipt.is_object()
        && receipt["project_id"] == state.project_id
        && receipt["review_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        && state.authorization_events.iter().any(|event| {
            event["kind"] == "integration_review_confirmed"
                && event["provenance"]["project_id"] == state.project_id
                && event["details"]["review_channel"] == *receipt
                && ["host-elicitation", "operator-cli"]
                    .contains(&event["operator"]["mechanism"].as_str().unwrap_or(""))
        })
}
