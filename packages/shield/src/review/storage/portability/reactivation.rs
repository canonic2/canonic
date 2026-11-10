//! Runtime-only restoration of exact private consent after authority repair.
use crate::review::{
    Checkpoint, Context, Error,
    model::State,
    registry_trust,
    storage::{Store, sources},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Preparation holds no authority writer. The final shared trust guard and
/// projection owner survive through the current-checkpoint CAS and durable save.
#[expect(
    clippy::too_many_lines,
    reason = "one restoration owns exact consent capture, cache preflight and trust/projection guards through the activation CAS and consent handoff"
)]
pub(in crate::review::storage) fn reactivate(
    store: &Store,
    context: &Context,
    state: State,
    checkpoint: Checkpoint<'_>,
) -> Result<(), Error> {
    checkpoint()?;
    state.check(context)?;
    if !registry_trust::may_reactivate(&state) {
        return Err(registry_trust::inactive_error(&state).unwrap_or(Error::StaleSnapshot));
    }
    let source = store.captured_candidate(
        state
            .candidates
            .get("project")
            .ok_or(Error::CandidateUnavailable)?,
    )?;
    let candidates = BTreeMap::from([("project".into(), source)]);
    let expected_digest = sources::map_digest(&state.candidates);
    if sources::map_digest(&candidates) != expected_digest {
        return Err(Error::StaleSnapshot);
    }
    let evaluator = crate::configuration::Evaluator::new_for_revision(
        crate::review::snapshots(&candidates)?,
        &state.policy_revision,
    )?;
    // A quick preflight avoids cache publication for still unavailable/revoked
    // authority. Drop this read lease before expensive cache preparation.
    registry_trust::recover_effective(&state.operator, evaluator.effective())?;
    match registry_trust::guard_effective(&state.operator, evaluator.effective()) {
        Ok(guard) => drop(guard),
        Err(error) => {
            record_failure(store, context, &state, &error, checkpoint)?;
            return Err(error);
        }
    }
    let (retained, _pins) = store.retain_candidates(&candidates)?;
    let prepared_consent = retained
        .get("project")
        .and_then(|reference| store.prepare_policy_consent(reference));
    let compiled = crate::review::cache::build_candidate(
        context,
        &retained,
        &state.policy_revision,
        &state.operator,
        &evaluator,
    )?;
    // No disposable descriptor fallback: the current codec and actual bytes
    // must load under this exact project identity before becoming Active.
    let verified = compiled.load(&state.project_root, &state.project_id)?;
    if verified.revision() != state.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    checkpoint()?;
    let projection = crate::review::publication::ProjectionLease::acquire(std::path::Path::new(
        &context.project_root,
    ))?;
    projection.check(context)?;
    let guard = match registry_trust::guard_effective(&state.operator, verified.effective()) {
        Ok(guard) => guard,
        Err(error) => {
            record_failure(store, context, &state, &error, checkpoint)?;
            return Err(error);
        }
    };
    let mut activation = None;
    let result = store.update_checkpoint(context, checkpoint, |current| {
        if let Some(active) = &current.active {
            return if active.revision() == state.policy_revision
                && sources::map_digest(&current.candidates) == expected_digest
            {
                activation = Some(active.activation_id().to_owned());
                Ok(json!({"already_current":true}))
            } else {
                Err(Error::StaleSnapshot)
            };
        }
        if !same_capture(current, &state) {
            return Err(Error::StaleSnapshot);
        }
        guard.bind_modes(&mut current.operator)?;
        crate::review::storage::stamp_policy_consent(
            &mut current.operator,
            prepared_consent.as_ref(),
        );
        current.candidates = retained.clone();
        let receipt = crate::review::runtime::publish_confirmed(
            current,
            context,
            None,
            compiled.clone(),
            "reviewed-policy-reuse",
        );
        activation = receipt["activation_id"].as_str().map(str::to_owned);
        Ok(receipt)
    });
    if let Err(error) = result {
        // A post-commit failure cannot erase a durable activation. Its exact
        // identity/bytes are checked again by the ordinary runtime loader.
        let committed = activation.as_ref().is_some_and(|id| {
            store.read_checkpoint().ok().is_some_and(|saved| {
                saved
                    .active
                    .as_ref()
                    .is_some_and(|active| active.activation_id() == id)
                    && saved.policy_revision == state.policy_revision
                    && sources::map_digest(&saved.candidates) == expected_digest
            })
        });
        if !committed {
            return Err(error);
        }
    }
    drop(guard);
    drop(projection);
    // The final context includes the acknowledged trust floors. Publish its
    // small consent index only after durable activation, outside authority.
    store.finish_policy_consent(prepared_consent, activation.as_deref());
    drop(state);
    Ok(())
}

fn same_capture(current: &State, captured: &State) -> bool {
    current.project_id == captured.project_id
        && current.project_root == captured.project_root
        && current.active.is_none()
        && current.policy_revision == captured.policy_revision
        && current.operator == captured.operator
        && sources::map_digest(&current.candidates) == sources::map_digest(&captured.candidates)
        && registry_trust::may_reactivate(current)
}

fn record_failure(
    store: &Store,
    context: &Context,
    state: &State,
    error: &Error,
    checkpoint: Checkpoint<'_>,
) -> Result<(), Error> {
    if !matches!(error, Error::RegistryRevoked { .. }) {
        return Ok(());
    }
    store
        .update_checkpoint(context, checkpoint, |current| {
            if !same_capture(current, state) {
                return Err(Error::StaleSnapshot);
            }
            registry_trust::record_inactive_reuse(current, context, error);
            Ok(Value::Null)
        })
        .map(|_| ())
}
