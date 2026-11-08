//! Admit the historical optional-cache activation without changing authority.
use super::{Active, Context, Error, State, confirmed_mechanism, digest};
use crate::review::{cache, storage::Store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Historical {
    activation_id: String,
    previous_activation_id: Option<String>,
    policy_revision: String,
    candidate_digest: String,
    candidates: BTreeMap<String, Value>,
    #[serde(default)]
    compiled: Option<cache::Artifact>,
    binding: String,
}

pub(in crate::review) fn restore(
    store: &Store,
    state: &mut State,
    raw: &Value,
) -> Result<(), Error> {
    restore_with_cache(store, state, raw, true)
}

pub(in crate::review) fn inspect(
    store: &Store,
    state: &mut State,
    raw: &Value,
) -> Result<(), Error> {
    restore_with_cache(store, state, raw, false)
}

fn restore_with_cache(
    store: &Store,
    state: &mut State,
    raw: &Value,
    publish: bool,
) -> Result<(), Error> {
    if raw.is_null() {
        return Ok(());
    }
    let old: Historical = serde_json::from_value(raw.clone())
        .map_err(|_| Error::Invalid("invalid historical activation"))?;
    let binding = crate::policy::fingerprint(
        json!([
            state.project_id,
            state.project_root,
            old.activation_id,
            old.previous_activation_id,
            old.policy_revision,
            old.candidate_digest
        ])
        .to_string()
        .as_bytes(),
    );
    if old.candidate_digest != digest(&old.candidates)
        || old.binding != binding
        || !state.authorization_events.iter().any(|event| {
            event["kind"] == "runtime_activated"
                && event["links"]["activation_id"] == old.activation_id
                && event["details"]["binding"] == old.binding
                && confirmed_mechanism(event)
        })
    {
        return Err(Error::Invalid(
            "historical activation authorization differs",
        ));
    }
    let compiled = if let Some(compiled) = old.compiled {
        compiled
    } else {
        let candidates = old
            .candidates
            .iter()
            .map(|(scope, value)| Ok((scope.clone(), store.captured_candidate(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let evaluator = crate::configuration::Evaluator::new_for_revision(
            crate::review::snapshots(&candidates)?,
            &old.policy_revision,
        )?;
        if evaluator.revision() != old.policy_revision {
            return Err(Error::Invalid(
                "historical policy revision differs from source",
            ));
        }
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        let described = cache::describe_candidate(
            &context,
            &candidates,
            &old.policy_revision,
            &state.operator,
            &evaluator,
        )?;
        // The source and activation are already verified. A missing or unwritable
        // derived cache must not make their checkpoint (and repair) unreadable.
        let compiled = if publish {
            match cache::build_candidate(
                &context,
                &candidates,
                &old.policy_revision,
                &state.operator,
                &evaluator,
            ) {
                Ok(compiled) => compiled,
                Err(Error::Io(_) | Error::CacheUnavailable) => described,
                Err(error) => return Err(error),
            }
        } else {
            described
        };
        let mut event = state.event("compiled_cache_prepared", &context,
            json!({"activation_id":old.activation_id}), json!({}),
            json!({"compiled_digest":compiled.digest,"policy_revision":old.policy_revision,"candidate_digest":old.candidate_digest,"migration":"optional-compiled-v1"}));
        event["operator"] =
            json!({"operator_id":null,"display_name":null,"mechanism":"derived-cache"});
        state.record(event);
        compiled
    };
    let active = Active {
        activation_id: old.activation_id,
        previous_activation_id: old.previous_activation_id,
        policy_revision: old.policy_revision,
        candidate_digest: old.candidate_digest,
        candidates: old.candidates,
        compiled,
        binding: old.binding,
    };
    active.validate_binding(state)?;
    state.active = Some(active);
    Ok(())
}
