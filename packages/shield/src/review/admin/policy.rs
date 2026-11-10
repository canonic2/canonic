//! Human-reviewed policy administration without terminal dependencies.
use crate::review::{
    Checkpoint, Context, Error, project,
    storage::{Store, history},
};
use serde_json::{Value, json};
use std::path::Path;

pub(in crate::review) struct History {
    before: Value,
    enabled: Option<bool>,
    limit: Option<usize>,
    purge: bool,
}

impl History {
    pub(in crate::review) fn new(
        context: &Context,
        enabled: Option<bool>,
        limit: Option<usize>,
        purge: bool,
    ) -> Result<Self, Error> {
        if limit.is_some_and(|v| !(1..=10000).contains(&v)) {
            return Err(Error::Invalid("history limit must be 1..10000"));
        }
        let state = project::checkpoint(context)?;
        Ok(Self {
            before: json!({"enabled":state.history_enabled,"limit":state.history_limit,"generation":state.history_generation}),
            enabled,
            limit,
            purge,
        })
    }

    pub(in crate::review) fn review(&self) -> Value {
        json!({"operation":"history settings","before":self.before,
            "after":{"enabled":self.enabled.unwrap_or(self.before["enabled"].as_bool().unwrap_or(true)),"limit":self.limit.map_or_else(||self.before["limit"].clone(),|v|json!(v))},"purge":self.purge})
    }

    pub(in crate::review) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        Store::open(Path::new(&context.project_root))?.update_checkpoint(context, checkpoint, |state| {
            if json!({"enabled":state.history_enabled,"limit":state.history_limit,"generation":state.history_generation}) != self.before { return Err(Error::StaleSnapshot); }
            if let Some(enabled) = self.enabled { state.history_enabled = enabled; }
            if let Some(limit) = self.limit { state.history_limit = limit; }
            if self.purge {
                state.events.clear();
                state.operator["history_retired_generations"] = json!(state.operator["bootstrap"]["history_recovery"]
                    .as_array().into_iter().flatten().take(2).cloned().collect::<Vec<_>>());
            }
            state.trim();
            state.history_control = true;
            if self.purge || self.limit.is_some() {
                state.history_generation = history::generation()?;
                state.history_owner = None;
                state.history_rewrite = Some(self.purge);
            }
            crate::review::configuration_changes::changed(state, context, "request history settings");
            Ok(json!({"schema_version":1,"enabled":state.history_enabled,"limit":state.history_limit,"purged":self.purge}))
        })
    }
}

pub(in crate::review) struct Reject {
    id: String,
    before: Value,
    revision: String,
    reason: Option<String>,
}

impl Reject {
    pub(in crate::review) fn new(
        context: &Context,
        id: &str,
        reason: Option<&str>,
    ) -> Result<Self, Error> {
        if reason.is_some_and(|r| r.len() > 4096) {
            return Err(crate::policy::Error::Limit.into());
        }
        let store = Store::open(Path::new(&context.project_root))?;
        let mut state = project::checkpoint(context)?;
        let proposal = state
            .proposals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(Error::Missing)?;
        store.hydrate_proposal(proposal)?;
        if proposal.status != "pending" {
            return Err(Error::Invalid("proposal is terminal or stale"));
        }
        if proposal.policy_revision != state.policy_revision {
            return Err(Error::StaleSnapshot);
        }
        Ok(Self {
            id: id.into(),
            before: proposal.public(false),
            revision: state.policy_revision,
            reason: reason.map(str::to_owned),
        })
    }

    pub(in crate::review) fn review(&self) -> Value {
        json!({"operation":"reject policy proposal","proposal":self.before,"reason":self.reason})
    }

    pub(in crate::review) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        let store = Store::open(Path::new(&context.project_root))?;
        store.update_checkpoint(context, checkpoint, |state| {
            if state.policy_revision != self.revision { return Err(Error::StaleSnapshot); }
            let index = state.proposals.iter().position(|p|p.id == self.id).ok_or(Error::Missing)?;
            store.hydrate_proposal(&mut state.proposals[index])?;
            if state.proposals[index].public(false) != self.before { return Err(Error::StaleSnapshot); }
            let operator = json!({"operator_id":null,"display_name":null,"mechanism":if context.surface == "cli" {"operator-cli"} else {"host-elicitation"}});
            state.proposals[index].status = "rejected".into();
            state.proposals[index].operator = Some(operator.clone());
            let mut event = state.event("proposal_rejected",context,json!({"proposal_id":self.id}),json!({"operator_reason":self.reason}),json!({"policy_revision":state.policy_revision}));
            event["operator"] = operator;
            state.record(event);
            Ok(state.proposals[index].public(false))
        })
    }
}
