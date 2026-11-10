//! Admit the historical optional-cache activation without changing authority.
use super::{Active, Context, Error, State, confirmed_mechanism, digest};
use crate::review::{cache, storage::Store};
use serde::{Deserialize, Serialize};
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
    compiled: Option<Value>,
    binding: String,
    #[serde(default = "super::legacy_binding_version")]
    binding_version: u8,
}

#[derive(Serialize)]
pub(in crate::review) struct LegacyPreview {
    pub(in crate::review) activation_id: Option<String>,
    policy_revision: String,
    candidate_digest: Option<String>,
    recorded_proof: &'static str,
    source: &'static str,
    model: &'static str,
    cache: &'static str,
}

pub(in crate::review) fn restore(
    store: &Store,
    state: &mut State,
    raw: &Value,
) -> Result<(), Error> {
    restore_with_cache(store, state, raw, true)
}

pub(in crate::review) fn inspect(
    _store: &Store,
    state: &mut State,
    raw: &Value,
) -> Result<(), Error> {
    let old = if raw.is_null() {
        None
    } else {
        Some(verified(state, raw)?)
    };
    state.legacy_preview = Some(LegacyPreview {
        activation_id: old.as_ref().map(|old| old.activation_id.clone()),
        policy_revision: old.as_ref().map_or_else(
            || state.policy_revision.clone(),
            |old| old.policy_revision.clone(),
        ),
        candidate_digest: old.as_ref().map(|old| old.candidate_digest.clone()),
        recorded_proof: if old.is_some() { "validated" } else { "absent" },
        source: "unchecked",
        model: "unchecked",
        cache: "unchecked",
    });
    Ok(())
}

fn verified(state: &State, raw: &Value) -> Result<Historical, Error> {
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
    if old.binding_version != 1
        || old.candidate_digest != digest(&old.candidates)
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
    Ok(old)
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
    let old = verified(state, raw)?;
    let compiled = if let Some(compiled) = old.compiled {
        serde_json::from_value::<cache::Artifact>(compiled)
            .map_err(|_| Error::Invalid("invalid historical activation"))?
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
        binding_version: old.binding_version,
    };
    active.validate_binding(state)?;
    state.active = Some(active);
    Ok(())
}
