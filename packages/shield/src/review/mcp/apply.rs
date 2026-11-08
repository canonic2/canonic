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
    views: &super::views::Views,
) -> Result<Value, Error> {
    let _trace = crate::diagnostics::trace::span("apply");
    let (before, id, source) = validate_with_views(context, args, Some(views))?;
    let proposal = before
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    if proposal.status == "applied" {
        let mut value = json!({"proposal_id":id,"status":"applied","policy_revision":proposal.receipt["policy_revision"],"receipt":proposal.receipt,"operator":proposal.operator});
        if before.operator["integration_publication"]["proposal_id"] == id
            && let Some(activation) = proposal.receipt["activation_id"].as_str()
        {
            crate::review::publication::resume_connections(
                context,
                Some(&id),
                activation,
                &mut value,
            );
        }
        return Ok(value);
    }
    let confirmation = crate::agents::review::for_context(context).confirmation();
    let connections_changed =
        crate::review::publication::mcp_changed(&proposal.base_candidate, &proposal.candidate)?;
    let mechanism = match confirmation {
        Confirmation::ToolPermission if connections_changed && confirmed => {
            HostConfirmation::Elicitation
        }
        Confirmation::ToolPermission if connections_changed => return Err(Error::ApprovalRequired),
        Confirmation::Form if confirmed => HostConfirmation::Elicitation,
        Confirmation::ToolPermission => HostConfirmation::ToolPermission,
        Confirmation::Form | Confirmation::Terminal => return Err(Error::ApprovalRequired),
    };
    checkpoint()?;
    let prepared = views.prepared(&source, &proposal.candidate_metadata);
    let receipt = crate::review::publication::commit_mcp(
        context,
        &before,
        proposal.candidate.clone(),
        proposal.candidate_metadata.clone(),
        &id,
        mechanism,
        crate::review::publication::Prepared {
            source: Some(&source),
            evaluator: prepared
                .as_ref()
                .map(|capture| capture.evaluator())
                .transpose()?,
        },
    )?;
    let mut result = json!({"proposal_id":id,"status":"applied","policy_revision":receipt["policy_revision"],"receipt":receipt,
        "operator":{"operator_id":null,"display_name":null,"mechanism":mechanism.mechanism()}});
    if connections_changed {
        result["connection_publication"] = receipt
            .get("connection_publication")
            .cloned()
            .unwrap_or_else(|| json!("not_required"));
        result["next_action"] = receipt.get("next_action").cloned().unwrap_or_else(|| {
            json!(if receipt.get("connection_publication").is_some() {
                "Connection configuration is active and its registered host bindings are published. Host loading remains subject to the agent's connection reload behavior."
            } else {
                "Connection configuration is active. No registered host entries needed publication; use mcp_setup to connect an agent if needed."
            })
        });
    }
    Ok(result)
}

pub(super) fn validate(context: &Context, args: &Value) -> Result<(State, String), Error> {
    validate_with_views(context, args, None).map(|(state, id, _)| (state, id))
}

fn validate_with_views(
    context: &Context,
    args: &Value,
    views: Option<&super::views::Views>,
) -> Result<(State, String, Value), Error> {
    let _trace = crate::diagnostics::trace::span("apply.validate");
    let adapter = crate::agents::review::for_context(context);
    // No model-supplied confirmation flag or human identity is accepted.
    if context.surface != "mcp" || adapter.confirmation() == Confirmation::Terminal {
        return Err(Error::ApprovalRequired);
    }
    let store = crate::review::storage::Store::open(std::path::Path::new(&context.project_root))?;
    let mut before = store.read_checkpoint()?;
    before.check(context)?;
    if before.candidates.len() != 1 || !before.candidates.contains_key("project") {
        return Err(Error::Invalid(
            "operator workflow requires project-only policy",
        ));
    }
    // Activation needs current authority and the selected review, never optional
    // history or every other proposal's source. Verify bytes before reusing a plan.
    let current_identity =
        crate::review::storage::sources::candidate_identity(&before.candidates["project"]);
    let same_active = before
        .active
        .as_ref()
        .is_some_and(|active| active.candidates == before.candidates);
    let prepared =
        views.and_then(|views| views.prepared(&before.candidates["project"], &before.operator));
    for candidate in before.candidates.values_mut() {
        *candidate = store.captured_candidate(candidate)?;
    }
    if !same_active && let Some(active) = &mut before.active {
        for candidate in active.candidates.values_mut() {
            *candidate = store.captured_candidate(candidate)?;
        }
    }
    let owned;
    let evaluator = if let Some(capture) = &prepared {
        capture.evaluator()?
    } else {
        owned = match crate::review::storage::Store::cached_evaluator(&before) {
            Some(evaluator) => evaluator,
            None => crate::configuration::Evaluator::new_for_revision(
                crate::review::snapshots(&before.candidates)?,
                &before.policy_revision,
            )?,
        };
        &owned
    };
    if evaluator.revision() != before.policy_revision {
        return Err(Error::Invalid(
            "review revision differs from retained policy",
        ));
    }
    // read_checkpoint validated activation authority before source expansion.
    // Identical retained references share the source verification above. A distinct
    // active snapshot must still validate its own source and evaluator revision.
    validate_active(&before, same_active, evaluator.revision())?;
    // Selection reads only proposal IDs; the exact comparison below regenerates
    // the selected proposal alone and binds its candidate, changes and review.
    let mut matches = before
        .proposals
        .iter_mut()
        .filter(|p| adapter.identifies(args, &p.id));
    let proposal = matches.next().ok_or(Error::Missing)?;
    if matches.next().is_some() {
        return Err(Error::Invalid("approval identifies multiple proposals"));
    }
    let binding = argument_binding(context, proposal);
    let source = proposal.candidate.clone();
    proposal.candidate = store.captured_candidate(&proposal.candidate)?;
    proposal.base_candidate =
        if crate::review::storage::sources::candidate_identity(&proposal.base_candidate)
            == current_identity
        {
            before.candidates["project"].clone()
        } else {
            store.captured_candidate(&proposal.base_candidate)?
        };
    let cached = views.and_then(|views| views.approval(&binding));
    let regenerated;
    let expected = if let Some(arguments) = cached {
        crate::diagnostics::trace::count("approval.presentation_hits", 1);
        arguments
    } else {
        regenerated = arguments(adapter, proposal);
        &regenerated
    };
    if args != expected {
        return Err(Error::Invalid(
            "approval arguments differ from the proposal. Retrieve it with policy_show using proposal_id, then copy apply_arguments unchanged to policy_apply",
        ));
    }
    if proposal.status == "applied" {
        let id = proposal.id.clone();
        return Ok((before, id, source));
    }
    validate_live(proposal, context, &before.policy_revision)?;
    crate::review::changes::historical::validate(&proposal.base_candidate, &proposal.candidate)?;
    let id = proposal.id.clone();
    crate::review::publication::validate_proposal_base(&before, &id)?;
    Ok((before, id, source))
}

fn validate_live(proposal: &Proposal, context: &Context, revision: &str) -> Result<(), Error> {
    if proposal.status == "stale" {
        return Err(Error::StaleSnapshot);
    }
    if proposal.status != "pending" {
        return Err(Error::Invalid(
            "this review is closed; inspect policy_receipt for its outcome and prepare a fresh change with a new idempotency_key if still authorized",
        ));
    }
    if proposal.policy_revision != revision {
        return Err(Error::StaleSnapshot);
    }
    if !crate::review::proposal_lifecycle::owns(proposal, context) {
        return Err(Error::Invalid(
            "this review belongs to another interaction; prepare a fresh change",
        ));
    }
    Ok(())
}

fn validate_active(before: &State, same_active: bool, revision: &str) -> Result<(), Error> {
    if let Some(active) = &before.active {
        if same_active {
            if active.revision() != revision {
                return Err(Error::Invalid(
                    "active revision differs from retained snapshot",
                ));
            }
        } else {
            active.validate(before)?;
        }
    }
    Ok(())
}

/// Every input to adapter presentation, except mutable status/receipt fields.
/// A cached rendering grants no authority: source integrity, current revision,
/// proposal status, exact arguments and locked publication checks remain required.
pub(super) fn argument_binding(context: &Context, proposal: &Proposal) -> String {
    crate::policy::fingerprint(
        json!([
            context.agent,
            proposal.id,
            crate::review::storage::sources::candidate_identity(&proposal.base_candidate),
            crate::review::storage::sources::candidate_identity(&proposal.candidate),
            proposal.base_metadata,
            proposal.candidate_metadata,
            proposal.comparison,
            proposal.reviewed_files,
        ])
        .to_string()
        .as_bytes(),
    )
}

fn arguments(adapter: &dyn ReviewAdapter, proposal: &Proposal) -> Value {
    let _trace = crate::diagnostics::trace::span("approval.arguments");
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

pub(super) fn present_proposal(context: &Context, proposal: &Proposal, data: &mut Value) {
    if proposal.status != "pending" || !crate::review::proposal_lifecycle::owns(proposal, context) {
        data.as_object_mut()
            .map(|object| object.remove("apply_arguments"));
        return;
    }
    let canonical = data["apply_arguments"].take();
    data["apply_arguments"] = present_arguments(
        crate::agents::review::for_context(context),
        proposal,
        &canonical,
    );
}

/// Build the response from the exact row already prepared by the workflow.
/// Formatting before persistence avoids a full project reload after saving it.
pub(super) fn proposal_result(context: &Context, proposal: &Proposal) -> Value {
    let mut data = proposal.public(false);
    if proposal.status != "pending" || !crate::review::proposal_lifecycle::owns(proposal, context) {
        data.as_object_mut()
            .map(|object| object.remove("apply_arguments"));
        data["next_action"] = json!(
            "This review is closed or belongs to another interaction. Inspect its outcome; if still authorized, prepare fresh with a new idempotency_key or supersedes_proposal_id. Never retry a declined offer without the user's request."
        );
        return data;
    }
    let canonical = data["apply_arguments"].take();
    if let Some(headline) = confirmation::headline(&canonical["changes"]) {
        data["change_summary"] = json!(headline);
    }
    let adapter = crate::agents::review::for_context(context);
    data["apply_arguments"] = present_arguments(adapter, proposal, &canonical);
    // Hosts may show the model only structured data; it carries the activation route.
    data["next_action"] = json!(adapter.guidance().proposed);
    data
}

/// Project canonical operator results onto the requesting adapter's wire view.
/// The persisted proposal and CLI output remain independent of host presentation.
pub(super) fn present(context: &Context, data: &mut Value) -> Result<(), Error> {
    let adapter = crate::agents::review::for_context(context);
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
