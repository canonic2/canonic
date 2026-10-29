//! Owns confirmed policy publication and independently retained runtime snapshots.
#[cfg(test)]
use super::Checkpoint;
use super::{Context, Error, model::State, snapshots, storage::Store};
use crate::{configuration, policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(crate) mod consumer;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Active {
    activation_id: String,
    previous_activation_id: Option<String>,
    policy_revision: String,
    candidate_digest: String,
    pub(in crate::review) candidates: BTreeMap<String, Value>,
    pub(in crate::review) compiled: super::cache::Artifact,
    binding: String,
}

fn digest(candidates: &BTreeMap<String, Value>) -> String {
    super::storage::sources::map_digest(candidates)
}

impl Active {
    pub(super) fn project_candidate(&self) -> Result<&Value, Error> {
        self.candidates
            .get("project")
            .ok_or(Error::Invalid("active project candidate missing"))
    }
    pub(super) fn activation_id(&self) -> &str {
        &self.activation_id
    }
    pub(super) fn matches(&self, identity: &consumer::SnapshotIdentity) -> bool {
        self.activation_id == identity.activation_id()
            && self.policy_revision == identity.policy_revision()
            && self.candidate_digest == identity.candidate_digest()
    }
    fn binding(&self, state: &State) -> String {
        policy::fingerprint(
            json!([
                state.project_id,
                state.project_root,
                self.activation_id,
                self.previous_activation_id,
                self.policy_revision,
                self.candidate_digest
            ])
            .to_string()
            .as_bytes(),
        )
    }

    pub(super) fn validate(&self, state: &State) -> Result<(), Error> {
        self.validate_binding(state)?;
        let evaluator = configuration::Evaluator::new(snapshots(&self.candidates)?)?;
        if evaluator.revision() != self.policy_revision {
            return Err(Error::Invalid(
                "active revision differs from retained snapshot",
            ));
        }
        Ok(())
    }

    /// Reuse validation of identical current bytes, while checking activation's
    /// own binding and revision. Different retained snapshots validate separately.
    pub(super) fn validate_evaluated(&self, state: &State, revision: &str) -> Result<(), Error> {
        if self.candidates != state.candidates {
            return self.validate(state);
        }
        self.validate_binding(state)?;
        if self.policy_revision != revision {
            return Err(Error::Invalid(
                "active revision differs from retained snapshot",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_binding(&self, state: &State) -> Result<(), Error> {
        if self.candidate_digest != digest(&self.candidates)
            || self.binding != self.binding(state)
            || !state.authorization_events.iter().any(|event| {
                event["kind"] == "runtime_activated"
                    && event["links"]["activation_id"] == self.activation_id
                    && event["details"]["binding"] == self.binding
                    && confirmed_mechanism(event)
            })
        {
            return Err(Error::Invalid(
                "active snapshot authorization binding differs",
            ));
        }
        if !state.authorization_events.iter().any(|event| {
            event["links"]["activation_id"] == self.activation_id
                && ["runtime_activated", "compiled_cache_prepared"]
                    .contains(&event["kind"].as_str().unwrap_or(""))
                && event["details"]["compiled_digest"] == self.compiled.digest
                && (confirmed_mechanism(event)
                    || (event["kind"] == "compiled_cache_prepared"
                        && event["operator"]["mechanism"] == "derived-cache"))
        }) {
            return Err(Error::Invalid(
                "compiled plan authorization binding differs",
            ));
        }
        Ok(())
    }

    pub(super) fn revision(&self) -> &str {
        &self.policy_revision
    }

    pub(in crate::review) fn metadata(&self) -> Value {
        json!({"activation_id":self.activation_id,"previous_activation_id":self.previous_activation_id,
            "policy_revision":self.policy_revision,"candidate_digest":self.candidate_digest})
    }
}

pub(super) fn status(context: &Context) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    Ok(
        json!({"schema_version":1,"mode":"local-runtime","enforcement":"not-installed",
        "project_id":state.project_id,"project_root":state.project_root,
        "review":{"policy_revision":state.policy_revision,"candidate_digest":digest(&state.candidates)},
        "active":state.active.as_ref().map(Active::metadata),
        "compiled_cache":state.active.as_ref().map(|a| a.compiled.diagnostics(&context.project_root, &a.candidate_digest, &a.policy_revision)),
        "authorization":state.active.as_ref().and_then(|active| state.authorization_events.iter().find(|event| event["links"]["activation_id"] == active.activation_id)),
        "pending_change":state.active.as_ref().is_none_or(|a| a.candidate_digest != digest(&state.candidates))}),
    )
}

/// Publish the already validated confirmed candidate inside its writer transaction.
pub(super) fn publish(
    state: &mut State,
    context: &Context,
    proposal: Option<&str>,
    compiled: super::cache::Artifact,
) -> Value {
    publish_confirmed(state, context, proposal, compiled, "operator-cli")
}

fn confirmed_mechanism(event: &Value) -> bool {
    ["operator-cli", "host-tool-permission", "host-elicitation"]
        .contains(&event["operator"]["mechanism"].as_str().unwrap_or(""))
}

pub(super) fn publish_confirmed(
    state: &mut State,
    context: &Context,
    proposal: Option<&str>,
    compiled: super::cache::Artifact,
    mechanism: &str,
) -> Value {
    let compiled_digest = compiled.digest.clone();
    let mut active = Active {
        activation_id: state.id("activation"),
        previous_activation_id: state.active.as_ref().map(|a| a.activation_id.clone()),
        policy_revision: state.policy_revision.clone(),
        candidate_digest: digest(&state.candidates),
        candidates: state.candidates.clone(),
        compiled,
        binding: String::new(),
    };
    active.binding = active.binding(state);
    let mut event = state.event("runtime_activated", context,
        json!({"activation_id":active.activation_id,"proposal_id":proposal}), json!({}),
        json!({"binding":active.binding,"policy_revision":active.policy_revision,"candidate_digest":active.candidate_digest,"compiled_digest":compiled_digest}));
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":mechanism});
    state.record(event);
    let mut result = active.metadata();
    result["committed"] = json!(true);
    state.active = Some(active);
    result
}

#[cfg(test)]
pub(super) fn publish_fixture(
    context: &Context,
    revision: &str,
    candidate_digest: &str,
    expected_activation: Option<&str>,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    if context.surface != "cli" {
        return Err(Error::ApprovalRequired);
    }
    Store::open(Path::new(&context.project_root))?.update(context, checkpoint, |state| {
        if revision != state.policy_revision || candidate_digest != digest(&state.candidates) {
            return Err(Error::Invalid(
                "stale activation candidate; inspect runtime status",
            ));
        }
        if let Some(active) = &state.active
            && active.policy_revision == revision
            && active.candidate_digest == candidate_digest
            && active.previous_activation_id.as_deref() == expected_activation
        {
            return Ok(active.metadata());
        }
        if state.active.as_ref().map(|a| a.activation_id.as_str()) != expected_activation {
            return Err(Error::Invalid(
                "stale activation predecessor; inspect runtime status",
            ));
        }
        // Validate the captured project under the lock before publication.
        let evaluator = configuration::Evaluator::new(snapshots(&state.candidates)?)?;
        if evaluator.revision() != revision {
            return Err(Error::Invalid("activation revision changed"));
        }
        checkpoint()?;
        let compiled = super::cache::build(state, None)?;
        publish(state, context, None, compiled);
        Ok(state.active.as_ref().expect("published fixture").metadata())
    })
}

#[cfg(test)]
mod enforcement_tests;
#[cfg(test)]
mod tests;
