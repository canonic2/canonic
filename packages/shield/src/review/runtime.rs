//! Owns the request consumer API and the runtime status view. Requests are
//! decided by the policy files; review state keeps only runtime records.
#[cfg(test)]
use super::storage::Store;
use super::{Context, Error, model::State};
use serde_json::{Value, json};
#[cfg(test)]
use std::path::Path;

pub(crate) mod consumer;

#[cfg(test)]
pub(crate) fn status(context: &Context) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    Ok(status_from(&state))
}

/// The identity requests are decided under: the current policy files.
#[cfg(test)]
pub(crate) fn current_identity(context: &Context) -> Value {
    let snapshot = consumer::load(context, &|| Ok(())).expect("current policy files");
    let identity = snapshot.identity();
    json!({"activation_id":identity.activation_id(),"policy_revision":identity.policy_revision(),
        "candidate_digest":identity.candidate_digest()})
}

pub(super) fn status_from(state: &State) -> Value {
    json!({"schema_version":1,"mode":"local-runtime","enforcement":"not-checked",
        "project_id":state.project_id,"project_root":state.project_root,
        // The policy files are the policy; only a live proposal is pending.
        "pending_change":state.proposals.iter().any(|proposal| proposal.status == "pending")})
}

#[cfg(test)]
mod enforcement_tests;
#[cfg(test)]
mod tests;
