//! Validates immutable proposals and applies registered host confirmation.
use super::{Checkpoint, Context, Error, confirmation};
use crate::review::publication::HostConfirmation;
use crate::{
    agents::review::{Confirmation, ReviewAdapter},
    review::model::{Proposal, State},
};
use serde_json::{Value, json};

pub(super) fn apply(
    context: &Context,
    args: &Value,
    checkpoint: Checkpoint<'_>,
    confirmed: bool,
) -> Result<Value, Error> {
    let (before, id) = validate(context, args)?;
    let proposal = before
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    if proposal.status == "applied" {
        return Ok(
            json!({"proposal_id":id,"status":"applied","policy_revision":proposal.receipt["policy_revision"],"receipt":proposal.receipt,"operator":proposal.operator}),
        );
    }
    let mechanism = match crate::agents::review::resolve(context.agent.as_deref()).confirmation() {
        Confirmation::Form if confirmed => HostConfirmation::Elicitation,
        Confirmation::ToolPermission => HostConfirmation::ToolPermission,
        Confirmation::Form | Confirmation::Terminal => return Err(Error::ApprovalRequired),
    };
    checkpoint()?;
    let receipt = crate::review::publication::commit_mcp(
        context,
        &before,
        proposal.candidate.clone(),
        proposal.candidate_metadata.clone(),
        &id,
        mechanism,
    )?;
    Ok(
        json!({"proposal_id":id,"status":"applied","policy_revision":receipt["policy_revision"],"receipt":receipt,
        "operator":{"operator_id":null,"display_name":null,"mechanism":mechanism.mechanism()}}),
    )
}

pub(super) fn validate(context: &Context, args: &Value) -> Result<(State, String), Error> {
    let adapter = crate::agents::review::resolve(context.agent.as_deref());
    // No model-supplied confirmation flag or human identity is accepted.
    if context.surface != "mcp" || adapter.confirmation() == Confirmation::Terminal {
        return Err(Error::ApprovalRequired);
    }
    let before = crate::review::project::load(context)?;
    // Selection reads only proposal IDs; the exact comparison below regenerates
    // the selected proposal alone and binds its candidate, changes and review.
    let mut matches = before
        .proposals
        .iter()
        .filter(|p| adapter.identifies(args, &p.id));
    let proposal = matches.next().ok_or(Error::Missing)?;
    if matches.next().is_some() || args != &arguments(adapter, proposal) {
        return Err(Error::Invalid(
            "approval arguments differ from the proposed change. Retrieve the proposal with policy_show using its proposal_id, then copy the returned apply_arguments object unchanged into policy_apply; do not reconstruct or summarize its fields",
        ));
    }
    if proposal.status == "applied" {
        let id = proposal.id.clone();
        return Ok((before, id));
    }
    if proposal.status != "pending" || proposal.policy_revision != before.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    crate::review::publication::validate_proposal_base(&before, &proposal.id)?;
    crate::review::publication::validate_mcp_change(&before, &proposal.candidate)?;
    let id = proposal.id.clone();
    Ok((before, id))
}

fn arguments(adapter: &dyn ReviewAdapter, proposal: &Proposal) -> Value {
    let canonical = proposal.apply_arguments();
    present_arguments(adapter, proposal, &canonical)
}

fn present_arguments(adapter: &dyn ReviewAdapter, proposal: &Proposal, canonical: &Value) -> Value {
    let mut value = if adapter.readable_arguments() {
        adapter.apply_arguments(canonical, &confirmation::summary(proposal, canonical))
    } else {
        adapter.apply_arguments(canonical, "")
    };
    crate::review::model::redact(&mut value);
    value
}

/// Build the response from the exact row already prepared by the workflow.
/// Formatting before persistence avoids a full project reload after saving it.
pub(super) fn proposal_result(context: &Context, proposal: &Proposal) -> Value {
    let mut data = proposal.public(false);
    let canonical = data["apply_arguments"].take();
    if let Some(headline) = confirmation::headline(&canonical["changes"]) {
        data["change_summary"] = json!(headline);
    }
    let adapter = crate::agents::review::resolve(context.agent.as_deref());
    data["apply_arguments"] = present_arguments(adapter, proposal, &canonical);
    // Hosts may show the model only structured data; it carries the activation route.
    data["next_action"] = json!(adapter.guidance().proposed);
    data
}

/// Project canonical operator results onto the requesting adapter's wire view.
/// The persisted proposal and CLI output remain independent of host presentation.
pub(super) fn present(context: &Context, data: &mut Value) -> Result<(), Error> {
    let adapter = crate::agents::review::resolve(context.agent.as_deref());
    if !adapter.readable_arguments() {
        return Ok(());
    }
    if data.get("apply_arguments").is_none() {
        return Ok(());
    }
    // Presentation is read-only: check enrollment and hydrate only the selected
    // retained proposal. Activation still uses validate's full policy checks.
    let store = crate::review::storage::Store::open(std::path::Path::new(&context.project_root))?;
    let mut state = store.read_checkpoint()?;
    state.check(context)?;
    let proposal = state
        .proposals
        .iter_mut()
        .find(|p| data["proposal_id"] == p.id)
        .ok_or(Error::Missing)?;
    proposal.candidate = store.captured_candidate(&proposal.candidate)?;
    proposal.base_candidate = store.captured_candidate(&proposal.base_candidate)?;
    data["apply_arguments"] = arguments(adapter, proposal);
    Ok(())
}
