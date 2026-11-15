//! Consumer identity and exact retained-record compatibility across maintenance.
use super::{Authority, Conversation, Error, Record, State, hash, json};
use crate::review::integrations::Registration;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub(super) fn scoped(registration: &Registration, agent: &str) -> Option<String> {
    let adapter = crate::agents::find(agent)?;
    if !registration.covers(agent) {
        return None;
    }
    let defaults = crate::review::mcp::default_approved();
    let tools: BTreeSet<_> = registration
        .approved_tools
        .as_deref()
        .unwrap_or(&defaults)
        .iter()
        .collect();
    let shared = adapter.spec().mcp_path.is_none() && registration.agents.contains_key(agent)
        || agent == "claude" && registration.interceptor.is_some();
    Some(format!(
        "agent-v1:{}",
        hash(&json!({
            "agent":agent,"binary":registration.binary,"native":registration.agents.get(agent),
            "connections":registration.connections.get(agent),"fallback":registration.ask_fallback.get(agent),
            "tools":tools,"entry":shared.then_some(&registration.entry),
            "legacy":(agent=="claude").then_some(&registration.interceptor),
            // The key predates Hub; Box definitions keep their earlier encoding.
            "box":adapter.supports_box_hooks().then_some(&registration.definition)
        }))
    ))
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Compatibility {
    records: BTreeMap<String, RecordBinding>,
    contexts: BTreeMap<String, ContextBinding>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordBinding {
    digest: String,
    agent: String,
    old_registration: String,
    scoped: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextBinding {
    key: String,
    agent: String,
    old_registration: String,
    scoped: String,
}

fn legacy(binding: &str) -> bool {
    binding.len() == 64 && binding.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_scoped(binding: &str) -> bool {
    binding.strip_prefix("agent-v1:").is_some_and(legacy)
}

impl Compatibility {
    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty() && self.contexts.is_empty()
    }

    pub(super) fn record_matches(&self, row: &Record, current: &BTreeMap<String, String>) -> bool {
        self.records.get(&row.id).is_some_and(|binding| {
            binding.digest == row.digest
                && binding.agent == row.session.agent
                && binding.old_registration == row.session.registration
                && current.get(&binding.agent) == Some(&binding.scoped)
        })
    }

    pub(super) fn context_matches(
        &self,
        key: &str,
        context: &Conversation,
        current: &BTreeMap<String, String>,
    ) -> bool {
        self.contexts
            .get(&context.reference)
            .is_some_and(|binding| {
                binding.key == key
                    && binding.agent == context.agent
                    && binding.old_registration == context.registration
                    && current.get(&binding.agent) == Some(&binding.scoped)
            })
    }

    pub(super) fn prune(
        &mut self,
        contexts: &BTreeMap<String, Conversation>,
        records: &BTreeMap<String, Record>,
    ) {
        self.records.retain(|id, binding| {
            records.get(id).is_some_and(|row| {
                binding.digest == row.digest
                    && binding.agent == row.session.agent
                    && binding.old_registration == row.session.registration
            })
        });
        self.contexts.retain(|reference, binding| {
            contexts.get(&binding.key).is_some_and(|context| {
                reference == &context.reference
                    && binding.agent == context.agent
                    && binding.old_registration == context.registration
            })
        });
    }

    pub(super) fn validate(
        &self,
        contexts: &BTreeMap<String, Conversation>,
        records: &BTreeMap<String, Record>,
    ) -> Result<(), Error> {
        if self.records.len() > records.len() || self.contexts.len() > contexts.len() {
            return Err(Error::Invalid(
                "session compatibility exceeds retained references",
            ));
        }
        for (id, binding) in &self.records {
            if !legacy(&binding.old_registration)
                || !valid_scoped(&binding.scoped)
                || records.get(id).is_none_or(|row| {
                    row.digest != binding.digest
                        || row.session.agent != binding.agent
                        || row.session.registration != binding.old_registration
                })
            {
                return Err(Error::Invalid(
                    "session record compatibility binding differs",
                ));
            }
        }
        for (reference, binding) in &self.contexts {
            if !legacy(&binding.old_registration)
                || !valid_scoped(&binding.scoped)
                || contexts.get(&binding.key).is_none_or(|context| {
                    context.reference != *reference
                        || context.agent != binding.agent
                        || context.registration != binding.old_registration
                })
            {
                return Err(Error::Invalid(
                    "session context compatibility binding differs",
                ));
            }
        }
        Ok(())
    }
}

/// The caller holds the required-state writer lock, before replacing integration metadata.
pub(in crate::review) fn preserve_integration(
    state: &mut State,
    before: Option<&Registration>,
    after: Option<&Registration>,
) -> Result<(), Error> {
    if crate::review::integrations::registration(&state.operator)?.as_ref() != before {
        return Err(Error::StaleSnapshot);
    }
    let authority = Authority::capture(state);
    let now = crate::review::clock::best_effort_ms();
    let unchanged: BTreeMap<_, _> = authority
        .registrations
        .iter()
        .filter_map(|(agent, old)| {
            after
                .and_then(|r| scoped(r, agent))
                .filter(|new| new == old)
                .map(|binding| (agent.as_str(), binding))
        })
        .collect();
    let mut compatibility = Compatibility::default();
    for (id, row) in &state.session_grants.records {
        if let Some(binding) = unchanged.get(row.session.agent.as_str())
            && legacy(&row.session.registration)
            && row.current_with(&authority)
            && ["pending", "active", "suspended", "declined"]
                .contains(&row.effective_with(&authority, now))
        {
            compatibility.records.insert(
                id.clone(),
                RecordBinding {
                    digest: row.digest.clone(),
                    agent: row.session.agent.clone(),
                    old_registration: row.session.registration.clone(),
                    scoped: binding.clone(),
                },
            );
        }
    }
    for (key, context) in &state.session_grants.contexts {
        if let Some(binding) = unchanged.get(context.agent.as_str())
            && legacy(&context.registration)
            && authority.context_matches(key, context)
            && now < context.created.saturating_add(super::RETAIN)
        {
            compatibility.contexts.insert(
                context.reference.clone(),
                ContextBinding {
                    key: key.clone(),
                    agent: context.agent.clone(),
                    old_registration: context.registration.clone(),
                    scoped: binding.clone(),
                },
            );
        }
    }
    state.session_grants.compatibility = compatibility;
    Ok(())
}
