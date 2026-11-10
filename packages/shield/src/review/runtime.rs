//! Owns confirmed policy publication and independently retained runtime snapshots.
#[cfg(test)]
use super::Checkpoint;
use super::{Context, Error, model::State, snapshots, storage::Store};
use crate::{configuration, policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

pub(crate) mod consumer;
mod upgrades;
pub(in crate::review) use upgrades::LegacyPreview;
pub(in crate::review) use upgrades::inspect as inspect_historical;
pub(in crate::review) use upgrades::restore as restore_historical;

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
    #[serde(
        default = "legacy_binding_version",
        skip_serializing_if = "is_legacy_binding"
    )]
    binding_version: u8,
}

fn legacy_binding_version() -> u8 {
    1
}
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "Serde skip predicate receives its field by reference"
)]
fn is_legacy_binding(version: &u8) -> bool {
    *version == 1
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
        self.matches_parts(
            identity.activation_id(),
            identity.policy_revision(),
            identity.candidate_digest(),
        )
    }
    /// The same identity check for a receipt's recorded identity fields.
    pub(super) fn matches_parts(&self, activation: &str, revision: &str, digest: &str) -> bool {
        self.activation_id == activation
            && self.policy_revision == revision
            && self.candidate_digest == digest
    }
    fn binding(&self, state: &State) -> String {
        if self.binding_version == 2 {
            return policy::fingerprint(
                json!([
                    "shield-policy-activation-v2",
                    state.policy_lineage(),
                    state.project_id,
                    self.activation_id,
                    self.previous_activation_id,
                    self.policy_revision,
                    self.candidate_digest
                ])
                .to_string()
                .as_bytes(),
            );
        }
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
        let evaluator = configuration::Evaluator::new_for_revision(
            snapshots(&self.candidates)?,
            &self.policy_revision,
        )?;
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
        if ![1, 2].contains(&self.binding_version)
            || self.candidate_digest != digest(&self.candidates)
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

    /// Only event-bound, size/schema/identity-checked coverage facts are usable.
    /// Missing historical metadata never causes source or evaluator preparation.
    pub(in crate::review) fn inspection<'a>(
        &'a self,
        state: &State,
    ) -> Option<&'a super::cache::Inspection> {
        let metadata = self.compiled.inspection()?;
        if !metadata.matches(&self.candidate_digest, &self.policy_revision) {
            return None;
        }
        let digest = self.compiled.inspection_digest()?;
        state
            .authorization_events
            .iter()
            .any(|event| {
                event["links"]["activation_id"] == self.activation_id
                    && ["runtime_activated", "compiled_cache_prepared"]
                        .contains(&event["kind"].as_str().unwrap_or(""))
                    && event["details"]["compiled_digest"] == self.compiled.digest
                    && event["details"]["inspection_digest"] == digest
                    && (confirmed_mechanism(event)
                        || (event["kind"] == "compiled_cache_prepared"
                            && event["operator"]["mechanism"] == "derived-cache"))
            })
            .then_some(metadata)
    }

    pub(in crate::review) fn metadata(&self) -> Value {
        json!({"activation_id":self.activation_id,"previous_activation_id":self.previous_activation_id,
            "policy_revision":self.policy_revision,"candidate_digest":self.candidate_digest})
    }
}

#[cfg(test)]
pub(crate) fn status(context: &Context) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    Ok(status_from(&state))
}

pub(super) fn status_from(state: &State) -> Value {
    if let Some(preview) = &state.legacy_preview {
        return json!({"schema_version":1,"mode":"historical-record-preview",
            "classification":if preview.activation_id.is_some() {"historical-reviewed-record"}else{"historical-unactivated-record"},
            "checkpoint_upgrade_pending":true,"policy_lineage":state.policy_lineage(),
            "project_id":state.project_id,"project_root":state.project_root,
            "review":{"policy_revision":state.policy_revision},"legacy_activation":preview,
            "active":null,"authorization":null,"pending_change":null,
            "compiled_cache":{"presence":"unchecked","compatibility":"unchecked","integrity":"unchecked"}});
    }
    let mut value = json!({"schema_version":1,"mode":"local-runtime","enforcement":"not-checked",
        "policy_lineage":state.policy_lineage(),
        "project_id":state.project_id,"project_root":state.project_root,
        "review":{"policy_revision":state.policy_revision,"candidate_digest":digest(&state.candidates)},
        "active":state.active.as_ref().map(Active::metadata),
        "compiled_cache":state.active.as_ref().map(|a| a.compiled.diagnostics(&state.project_root, &a.candidate_digest, &a.policy_revision)),
        "authorization":state.active.as_ref().and_then(|active| state.authorization_events.iter().find(|event| event["links"]["activation_id"] == active.activation_id)),
        "pending_change":state.active.as_ref().is_none_or(|a| a.candidate_digest != digest(&state.candidates))});
    if super::registry_trust::inactive_reviewed(state) {
        value["reviewed_consent"] = state.operator["policy_consent"].clone();
        value["next_action"] = state.operator["policy_consent"]["next_action"].clone();
    }
    value
}

/// Publish the already validated confirmed candidate inside its writer transaction.
#[cfg(test)]
pub(super) fn publish(
    state: &mut State,
    context: &Context,
    proposal: Option<&str>,
    compiled: super::cache::Artifact,
) -> Value {
    publish_confirmed(state, context, proposal, compiled, "operator-cli")
}

fn confirmed_mechanism(event: &Value) -> bool {
    [
        "operator-cli",
        "host-tool-permission",
        "host-elicitation",
        "reviewed-policy-reuse",
    ]
    .contains(&event["operator"]["mechanism"].as_str().unwrap_or(""))
}

pub(super) fn publish_confirmed(
    state: &mut State,
    context: &Context,
    proposal: Option<&str>,
    compiled: super::cache::Artifact,
    mechanism: &str,
) -> Value {
    if let Some(metadata) = state.operator.as_object_mut() {
        metadata.remove("policy_consent");
    }
    state.remember_policy = true;
    if state.operator.get("policy_lineage").is_none() {
        state.operator["policy_lineage"] = json!(state.project_id);
    }
    let compiled_digest = compiled.digest.clone();
    let inspection_digest = compiled.inspection_digest();
    let mut active = Active {
        activation_id: state.id("activation"),
        previous_activation_id: state.active.as_ref().map(|a| a.activation_id.clone()),
        policy_revision: state.policy_revision.clone(),
        candidate_digest: digest(&state.candidates),
        candidates: state.candidates.clone(),
        compiled,
        binding: String::new(),
        binding_version: 2,
    };
    active.binding = active.binding(state);
    let mut event = state.event("runtime_activated", context,
        json!({"activation_id":active.activation_id,"proposal_id":proposal}), json!({}),
        json!({"binding":active.binding,"policy_revision":active.policy_revision,"candidate_digest":active.candidate_digest,"compiled_digest":compiled_digest,"inspection_digest":inspection_digest}));
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":mechanism});
    state.record(event);
    let mut result = active.metadata();
    result["committed"] = json!(true);
    state.active = Some(active);
    if mechanism == "reviewed-policy-reuse" {
        let receipt = json!({"project_id":state.project_id,"project_root":state.project_root});
        state.operator["policy_reuse"] = receipt.clone();
        let mut event = state.event(
            "policy_reused",
            context,
            json!({}),
            json!({}),
            json!({"receipt":receipt}),
        );
        event["operator"] = json!({"mechanism":"reviewed-policy-reuse"});
        state.record(event);
    }
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
    let store = Store::open(Path::new(&context.project_root))?;
    let (before, evaluator) = store.read_current_evaluated()?;
    if evaluator.revision() != revision || candidate_digest != digest(&before.candidates) {
        return Err(Error::Invalid(
            "stale activation candidate; inspect runtime status",
        ));
    }
    let compiled = super::cache::build_candidate(
        context,
        &before.candidates,
        revision,
        &before.operator,
        &evaluator,
    )?;
    let (retained, _source_pins) = store.retain_candidates(&before.candidates)?;
    let prepared_consent = store.prepare_policy_consent(&retained["project"]);
    let expected_metadata = before.operator.clone();
    drop(before);
    let result = store.update_checkpoint(context, checkpoint, |state| {
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
        if state.operator != expected_metadata {
            return Err(Error::StaleSnapshot);
        }
        if state.active.as_ref().map(|a| a.activation_id.as_str()) != expected_activation {
            return Err(Error::Invalid(
                "stale activation predecessor; inspect runtime status",
            ));
        }
        checkpoint()?;
        state.candidates = retained;
        super::storage::stamp_policy_consent(&mut state.operator, prepared_consent.as_ref());
        publish(state, context, None, compiled);
        Ok(state.active.as_ref().expect("published fixture").metadata())
    });
    if let Ok(receipt) = &result {
        store.finish_policy_consent(prepared_consent, receipt["activation_id"].as_str());
    }
    result
}

#[cfg(test)]
mod enforcement_tests;
#[cfg(test)]
mod tests;
